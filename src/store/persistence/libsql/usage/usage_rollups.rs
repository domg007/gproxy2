//! Usage-rollup ops for the libSQL edge backend (accumulate by dimension bucket).

use serde_json::Value;

use crate::store::libsql::{LibsqlClient, arg_integer, arg_text};
use crate::store::persistence::libsql::row::{
    Row, col_decimal, col_i64, col_opt_i64, col_opt_str, col_str,
};
use crate::store::persistence::libsql::util::{
    arg_opt_i64, arg_opt_text, exec, last_rowid, now_secs, query, query_one,
};
use crate::store::persistence::records::{UsageRollup, UsageRollupInput};

const COLS: &str = "id, granularity, bucket_start, provider_id, org_id, team_id, user_id, \
     route_name, model, requests, input_tokens, output_tokens, cost, created_at, updated_at";

fn decode(row: &Row) -> anyhow::Result<UsageRollup> {
    Ok(UsageRollup {
        id: col_i64(row, 0)?,
        granularity: col_str(row, 1)?,
        bucket_start: col_i64(row, 2)?,
        provider_id: col_opt_i64(row, 3)?,
        org_id: col_opt_i64(row, 4)?,
        team_id: col_opt_i64(row, 5)?,
        user_id: col_opt_i64(row, 6)?,
        route_name: col_opt_str(row, 7)?,
        model: col_opt_str(row, 8)?,
        requests: col_i64(row, 9)?,
        input_tokens: col_i64(row, 10)?,
        output_tokens: col_i64(row, 11)?,
        cost: col_decimal(row, 12)?,
        created_at: col_i64(row, 13)?,
        updated_at: col_i64(row, 14)?,
    })
}

/// Append an `<col> = ?`/`<col> IS NULL` predicate for an optional i64 dimension.
fn push_opt_i64(sql: &mut String, args: &mut Vec<Value>, col: &str, v: Option<i64>) {
    match v {
        Some(n) => {
            sql.push_str(&format!(" AND {col} = ?"));
            args.push(arg_integer(n));
        }
        None => sql.push_str(&format!(" AND {col} IS NULL")),
    }
}

fn push_opt_text(sql: &mut String, args: &mut Vec<Value>, col: &str, v: Option<&str>) {
    match v {
        Some(s) => {
            sql.push_str(&format!(" AND {col} = ?"));
            args.push(arg_text(s));
        }
        None => sql.push_str(&format!(" AND {col} IS NULL")),
    }
}

async fn find_bucket_id(
    client: &LibsqlClient,
    input: &UsageRollupInput,
) -> anyhow::Result<Option<i64>> {
    let mut sql =
        String::from("SELECT id FROM usage_rollups WHERE granularity = ? AND bucket_start = ?");
    let mut args: Vec<Value> = vec![
        arg_text(&input.granularity),
        arg_integer(input.bucket_start),
    ];
    push_opt_i64(&mut sql, &mut args, "provider_id", input.provider_id);
    push_opt_i64(&mut sql, &mut args, "org_id", input.org_id);
    push_opt_i64(&mut sql, &mut args, "team_id", input.team_id);
    push_opt_i64(&mut sql, &mut args, "user_id", input.user_id);
    push_opt_text(
        &mut sql,
        &mut args,
        "route_name",
        input.route_name.as_deref(),
    );
    push_opt_text(&mut sql, &mut args, "model", input.model.as_deref());

    query_one(client, &sql, &args)
        .await?
        .as_ref()
        .map(|r| col_i64(r, 0))
        .transpose()
}

async fn get(client: &LibsqlClient, id: i64) -> anyhow::Result<Option<UsageRollup>> {
    query_one(
        client,
        &format!("SELECT {COLS} FROM usage_rollups WHERE id = ?"),
        &[arg_integer(id)],
    )
    .await?
    .as_ref()
    .map(decode)
    .transpose()
}

pub async fn add(client: &LibsqlClient, input: UsageRollupInput) -> anyhow::Result<UsageRollup> {
    let now = now_secs();

    let id = match find_bucket_id(client, &input).await? {
        Some(id) => {
            let existing = get(client, id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("rollup bucket vanished"))?;
            let cost = existing.cost + input.cost;
            exec(
                client,
                "UPDATE usage_rollups SET requests=?, input_tokens=?, output_tokens=?, cost=?, \
                 updated_at=? WHERE id=?",
                &[
                    arg_integer(existing.requests + input.requests),
                    arg_integer(existing.input_tokens + input.input_tokens),
                    arg_integer(existing.output_tokens + input.output_tokens),
                    arg_text(&cost.to_string()),
                    arg_integer(now),
                    arg_integer(id),
                ],
            )
            .await?;
            id
        }
        None => {
            let qr = client
                .execute(
                    "INSERT INTO usage_rollups \
                     (granularity, bucket_start, provider_id, org_id, team_id, user_id, \
                      route_name, model, requests, input_tokens, output_tokens, cost, \
                      created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    &[
                        arg_text(&input.granularity),
                        arg_integer(input.bucket_start),
                        arg_opt_i64(input.provider_id),
                        arg_opt_i64(input.org_id),
                        arg_opt_i64(input.team_id),
                        arg_opt_i64(input.user_id),
                        arg_opt_text(input.route_name.as_deref()),
                        arg_opt_text(input.model.as_deref()),
                        arg_integer(input.requests),
                        arg_integer(input.input_tokens),
                        arg_integer(input.output_tokens),
                        arg_text(&input.cost.to_string()),
                        arg_integer(now),
                        arg_integer(now),
                    ],
                )
                .await
                .map_err(|e| anyhow::anyhow!("libsql insert usage_rollup: {e}"))?;
            last_rowid(&qr)?
        }
    };

    get(client, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("usage_rollup vanished after add"))
}

pub async fn list(
    client: &LibsqlClient,
    granularity: &str,
    from: i64,
    to: i64,
) -> anyhow::Result<Vec<UsageRollup>> {
    query(
        client,
        &format!(
            "SELECT {COLS} FROM usage_rollups WHERE granularity = ? AND bucket_start >= ? \
             AND bucket_start <= ? ORDER BY bucket_start ASC"
        ),
        &[arg_text(granularity), arg_integer(from), arg_integer(to)],
    )
    .await?
    .iter()
    .map(decode)
    .collect()
}
