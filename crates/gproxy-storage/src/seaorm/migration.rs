use sea_orm::ActiveValue::Set;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};
use time::OffsetDateTime;

use super::entities::{credential_statuses, credentials, providers, upstream_requests, usages};

const LEGACY_CHANNEL: &str = "claude";
const CURRENT_CHANNEL: &str = "anthropic";
const LEGACY_CREDENTIAL_KIND: &str = "builtin/claude";
const CURRENT_CREDENTIAL_KIND: &str = "builtin/anthropic";

#[derive(Debug, Default)]
struct MigrationStats {
    providers_renamed: u64,
    duplicate_providers_merged: u64,
    provider_refs_reassigned: u64,
    provider_settings_rewritten: u64,
    credentials_rewritten: u64,
    statuses_rewritten: u64,
    statuses_merged: u64,
}

impl MigrationStats {
    fn changed(&self) -> bool {
        self.providers_renamed > 0
            || self.duplicate_providers_merged > 0
            || self.provider_refs_reassigned > 0
            || self.provider_settings_rewritten > 0
            || self.credentials_rewritten > 0
            || self.statuses_rewritten > 0
            || self.statuses_merged > 0
    }
}

pub async fn run(db: &DatabaseConnection) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let stats = migrate_legacy_claude_channel(&txn).await?;
    txn.commit().await?;

    if stats.changed() {
        eprintln!(
            "storage: migrated legacy claude channel data to anthropic (providers_renamed={}, duplicate_providers_merged={}, provider_refs_reassigned={}, provider_settings_rewritten={}, credentials_rewritten={}, statuses_rewritten={}, statuses_merged={})",
            stats.providers_renamed,
            stats.duplicate_providers_merged,
            stats.provider_refs_reassigned,
            stats.provider_settings_rewritten,
            stats.credentials_rewritten,
            stats.statuses_rewritten,
            stats.statuses_merged,
        );
    }

    Ok(())
}

async fn migrate_legacy_claude_channel<C: ConnectionTrait>(
    conn: &C,
) -> Result<MigrationStats, DbErr> {
    let mut stats = MigrationStats::default();
    let now = OffsetDateTime::now_utc();

    migrate_providers(conn, now, &mut stats).await?;
    migrate_credentials(conn, now, &mut stats).await?;
    migrate_credential_statuses(conn, now, &mut stats).await?;

    Ok(stats)
}

async fn migrate_providers<C: ConnectionTrait>(
    conn: &C,
    now: OffsetDateTime,
    stats: &mut MigrationStats,
) -> Result<(), DbErr> {
    let provider_rows = providers::Entity::find()
        .filter(providers::Column::Channel.is_in([LEGACY_CHANNEL, CURRENT_CHANNEL]))
        .order_by_asc(providers::Column::Id)
        .all(conn)
        .await?;

    let canonical_legacy = provider_rows
        .iter()
        .find(|row| row.channel == LEGACY_CHANNEL && row.id == 2)
        .cloned()
        .or_else(|| {
            provider_rows
                .iter()
                .filter(|row| row.channel == LEGACY_CHANNEL)
                .min_by_key(|row| row.id)
                .cloned()
        });

    if let Some(canonical) = canonical_legacy {
        for duplicate in provider_rows.iter().filter(|row| row.id != canonical.id) {
            stats.provider_refs_reassigned +=
                reassign_provider_refs(conn, duplicate.id, canonical.id, now).await?;
            providers::Entity::delete_by_id(duplicate.id)
                .exec(conn)
                .await?;
            stats.duplicate_providers_merged += 1;
        }

        let (rewritten_settings, settings_changed) =
            rewrite_provider_settings_json(canonical.settings_json.clone());
        let mut active = canonical.into_active_model();
        let mut changed = false;

        if active.channel.as_ref() != CURRENT_CHANNEL {
            active.channel = Set(CURRENT_CHANNEL.to_string());
            stats.providers_renamed += 1;
            changed = true;
        }
        if active.name.as_ref() == LEGACY_CHANNEL {
            active.name = Set(CURRENT_CHANNEL.to_string());
            changed = true;
        }
        if settings_changed {
            active.settings_json = Set(rewritten_settings);
            stats.provider_settings_rewritten += 1;
            changed = true;
        }

        if changed {
            active.updated_at = Set(now);
            active.update(conn).await?;
        }
    } else {
        for row in provider_rows
            .into_iter()
            .filter(|row| row.channel == CURRENT_CHANNEL)
        {
            let (rewritten_settings, settings_changed) =
                rewrite_provider_settings_json(row.settings_json.clone());
            if !settings_changed {
                continue;
            }
            let mut active = row.into_active_model();
            active.settings_json = Set(rewritten_settings);
            active.updated_at = Set(now);
            active.update(conn).await?;
            stats.provider_settings_rewritten += 1;
        }
    }

    Ok(())
}

async fn reassign_provider_refs<C: ConnectionTrait>(
    conn: &C,
    from_provider_id: i64,
    to_provider_id: i64,
    now: OffsetDateTime,
) -> Result<u64, DbErr> {
    let mut rows_affected = 0;

    rows_affected += credentials::Entity::update_many()
        .set(credentials::ActiveModel {
            provider_id: Set(to_provider_id),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(credentials::Column::ProviderId.eq(from_provider_id))
        .exec(conn)
        .await?
        .rows_affected;

    rows_affected += upstream_requests::Entity::update_many()
        .set(upstream_requests::ActiveModel {
            provider_id: Set(Some(to_provider_id)),
            ..Default::default()
        })
        .filter(upstream_requests::Column::ProviderId.eq(from_provider_id))
        .exec(conn)
        .await?
        .rows_affected;

    rows_affected += usages::Entity::update_many()
        .set(usages::ActiveModel {
            provider_id: Set(Some(to_provider_id)),
            ..Default::default()
        })
        .filter(usages::Column::ProviderId.eq(from_provider_id))
        .exec(conn)
        .await?
        .rows_affected;

    Ok(rows_affected)
}

async fn migrate_credentials<C: ConnectionTrait>(
    conn: &C,
    now: OffsetDateTime,
    stats: &mut MigrationStats,
) -> Result<(), DbErr> {
    let rows = credentials::Entity::find()
        .filter(credentials::Column::Kind.eq(LEGACY_CREDENTIAL_KIND))
        .all(conn)
        .await?;

    for row in rows {
        let (rewritten_secret, secret_changed) =
            rewrite_credential_secret_json(row.secret_json.clone());
        let mut active = row.into_active_model();
        let mut changed = false;

        if active.kind.as_ref() != CURRENT_CREDENTIAL_KIND {
            active.kind = Set(CURRENT_CREDENTIAL_KIND.to_string());
            changed = true;
        }
        if secret_changed {
            active.secret_json = Set(rewritten_secret);
            changed = true;
        }

        if changed {
            active.updated_at = Set(now);
            active.update(conn).await?;
            stats.credentials_rewritten += 1;
        }
    }

    Ok(())
}

async fn migrate_credential_statuses<C: ConnectionTrait>(
    conn: &C,
    now: OffsetDateTime,
    stats: &mut MigrationStats,
) -> Result<(), DbErr> {
    use std::collections::HashMap;

    let current_rows = credential_statuses::Entity::find()
        .filter(credential_statuses::Column::Channel.eq(CURRENT_CHANNEL))
        .all(conn)
        .await?;
    let mut current_by_credential_id = current_rows
        .into_iter()
        .map(|row| (row.credential_id, row))
        .collect::<HashMap<_, _>>();

    let legacy_rows = credential_statuses::Entity::find()
        .filter(credential_statuses::Column::Channel.eq(LEGACY_CHANNEL))
        .all(conn)
        .await?;

    for legacy in legacy_rows {
        if let Some(current) = current_by_credential_id.get(&legacy.credential_id).cloned() {
            if prefer_legacy_status(&legacy, &current) {
                let mut active = current.into_active_model();
                active.health_kind = Set(legacy.health_kind.clone());
                active.health_json = Set(legacy.health_json.clone());
                active.checked_at = Set(legacy.checked_at);
                active.last_error = Set(legacy.last_error.clone());
                active.updated_at = Set(now);
                let updated = active.update(conn).await?;
                current_by_credential_id.insert(updated.credential_id, updated);
            }
            credential_statuses::Entity::delete_by_id(legacy.id)
                .exec(conn)
                .await?;
            stats.statuses_merged += 1;
            continue;
        }

        let mut active = legacy.into_active_model();
        active.channel = Set(CURRENT_CHANNEL.to_string());
        active.updated_at = Set(now);
        let updated = active.update(conn).await?;
        current_by_credential_id.insert(updated.credential_id, updated);
        stats.statuses_rewritten += 1;
    }

    Ok(())
}

fn prefer_legacy_status(
    legacy: &credential_statuses::Model,
    current: &credential_statuses::Model,
) -> bool {
    match (legacy.checked_at, current.checked_at) {
        (Some(legacy_checked), Some(current_checked)) if legacy_checked != current_checked => {
            return legacy_checked > current_checked;
        }
        (Some(_), None) => return true,
        (None, Some(_)) => return false,
        _ => {}
    }

    let legacy_score = status_detail_score(legacy);
    let current_score = status_detail_score(current);
    if legacy_score != current_score {
        return legacy_score > current_score;
    }

    legacy.updated_at > current.updated_at
}

fn status_detail_score(model: &credential_statuses::Model) -> i32 {
    let mut score = 0;
    if model.checked_at.is_some() {
        score += 4;
    }
    if model.health_kind != "healthy" {
        score += 2;
    }
    if model.health_json.is_some() {
        score += 1;
    }
    if model
        .last_error
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        score += 1;
    }
    score
}

fn rewrite_provider_settings_json(value: serde_json::Value) -> (serde_json::Value, bool) {
    let mut value = value;
    let Some(object) = value.as_object_mut() else {
        return (value, false);
    };

    let changed = move_json_key(object, "claude_prelude_text", "anthropic_prelude_text")
        | move_json_key(
            object,
            "claude_append_beta_query",
            "anthropic_append_beta_query",
        )
        | move_json_key(
            object,
            "claude_extra_beta_headers",
            "anthropic_extra_beta_headers",
        );

    (value, changed)
}

fn rewrite_credential_secret_json(value: serde_json::Value) -> (serde_json::Value, bool) {
    let mut value = value;
    let Some(root) = value.as_object_mut() else {
        return (value, false);
    };
    let Some(builtin) = root.get_mut("Builtin") else {
        return (value, false);
    };
    let Some(builtin_object) = builtin.as_object_mut() else {
        return (value, false);
    };

    let changed = move_json_key(builtin_object, "Claude", "Anthropic");
    (value, changed)
}

fn move_json_key(
    object: &mut serde_json::Map<String, serde_json::Value>,
    from: &str,
    to: &str,
) -> bool {
    let Some(value) = object.remove(from) else {
        return false;
    };
    object.entry(to.to_string()).or_insert(value);
    true
}

#[cfg(test)]
mod tests {
    use sea_orm::ActiveValue::Set;
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};
    use time::Duration;

    use super::*;
    use crate::SeaOrmStorage;

    #[tokio::test(flavor = "current_thread")]
    async fn migrates_legacy_claude_provider_and_related_rows() {
        let storage = SeaOrmStorage::connect("sqlite::memory:", None)
            .await
            .expect("connect sqlite memory");
        storage.sync().await.expect("sync schema");
        let db = storage.connection();
        let now = OffsetDateTime::now_utc();

        providers::ActiveModel {
            id: Set(2),
            name: Set("claude".to_string()),
            channel: Set("claude".to_string()),
            settings_json: Set(serde_json::json!({
                "base_url": "https://api.anthropic.com",
                "claude_prelude_text": "legacy prelude",
                "claude_append_beta_query": true,
                "claude_extra_beta_headers": ["beta-1"]
            })),
            dispatch_json: Set(serde_json::json!({})),
            enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .expect("insert legacy provider");

        providers::ActiveModel {
            id: Set(12),
            name: Set("anthropic".to_string()),
            channel: Set("anthropic".to_string()),
            settings_json: Set(serde_json::json!({
                "base_url": "https://api.anthropic.com/v2"
            })),
            dispatch_json: Set(serde_json::json!({})),
            enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now + Duration::minutes(5)),
        }
        .insert(db)
        .await
        .expect("insert duplicate anthropic provider");

        credentials::ActiveModel {
            id: Set(101),
            provider_id: Set(2),
            name: Set(Some("legacy-key".to_string())),
            kind: Set("builtin/claude".to_string()),
            settings_json: Set(None),
            secret_json: Set(serde_json::json!({
                "Builtin": {
                    "Claude": {
                        "api_key": "sk-ant-legacy"
                    }
                }
            })),
            enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .expect("insert legacy credential");

        credentials::ActiveModel {
            id: Set(202),
            provider_id: Set(12),
            name: Set(Some("new-key".to_string())),
            kind: Set("builtin/anthropic".to_string()),
            settings_json: Set(None),
            secret_json: Set(serde_json::json!({
                "Builtin": {
                    "Anthropic": {
                        "api_key": "sk-ant-new"
                    }
                }
            })),
            enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .expect("insert duplicate provider credential");

        credential_statuses::ActiveModel {
            id: Set(301),
            credential_id: Set(101),
            channel: Set("claude".to_string()),
            health_kind: Set("dead".to_string()),
            health_json: Set(None),
            checked_at: Set(Some(now + Duration::minutes(10))),
            last_error: Set(Some("401".to_string())),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .expect("insert legacy status");

        credential_statuses::ActiveModel {
            id: Set(302),
            credential_id: Set(101),
            channel: Set("anthropic".to_string()),
            health_kind: Set("healthy".to_string()),
            health_json: Set(None),
            checked_at: Set(None),
            last_error: Set(None),
            updated_at: Set(now - Duration::minutes(1)),
        }
        .insert(db)
        .await
        .expect("insert duplicate anthropic status");

        upstream_requests::ActiveModel {
            trace_id: Set(401),
            downstream_trace_id: Set(None),
            at: Set(now),
            internal: Set(false),
            provider_id: Set(Some(12)),
            credential_id: Set(Some(202)),
            request_method: Set("POST".to_string()),
            request_headers_json: Set(serde_json::json!({})),
            request_url: Set(Some("https://api.anthropic.com/v1/messages".to_string())),
            request_body: Set(None),
            response_status: Set(Some(200)),
            response_headers_json: Set(serde_json::json!({})),
            response_body: Set(None),
            created_at: Set(now),
        }
        .insert(db)
        .await
        .expect("insert upstream request");

        usages::ActiveModel {
            trace_id: Set(501),
            downstream_trace_id: Set(None),
            at: Set(now),
            provider_id: Set(Some(12)),
            credential_id: Set(Some(202)),
            user_id: Set(None),
            user_key_id: Set(None),
            operation: Set("generate".to_string()),
            protocol: Set("Claude".to_string()),
            model: Set(Some("claude-4-sonnet".to_string())),
            input_tokens: Set(Some(10)),
            output_tokens: Set(Some(20)),
            cache_read_input_tokens: Set(None),
            cache_creation_input_tokens: Set(None),
            cache_creation_input_tokens_5min: Set(None),
            cache_creation_input_tokens_1h: Set(None),
            created_at: Set(now),
        }
        .insert(db)
        .await
        .expect("insert usage");

        run(db).await.expect("run migration");

        let provider_rows = providers::Entity::find()
            .filter(providers::Column::Channel.eq("anthropic"))
            .all(db)
            .await
            .expect("list migrated providers");
        assert_eq!(provider_rows.len(), 1);
        let provider = &provider_rows[0];
        assert_eq!(provider.id, 2);
        assert_eq!(provider.name, "anthropic");
        assert_eq!(provider.channel, "anthropic");
        assert_eq!(
            provider.settings_json["anthropic_prelude_text"],
            "legacy prelude"
        );
        assert_eq!(provider.settings_json["anthropic_append_beta_query"], true);
        assert_eq!(
            provider.settings_json["anthropic_extra_beta_headers"],
            serde_json::json!(["beta-1"])
        );
        assert!(provider.settings_json.get("claude_prelude_text").is_none());

        let credential_rows = credentials::Entity::find()
            .order_by_asc(credentials::Column::Id)
            .all(db)
            .await
            .expect("list migrated credentials");
        assert_eq!(credential_rows.len(), 2);
        let legacy_credential = credential_rows
            .iter()
            .find(|row| row.id == 101)
            .expect("legacy credential exists");
        assert_eq!(legacy_credential.provider_id, 2);
        assert_eq!(legacy_credential.kind, "builtin/anthropic");
        assert_eq!(
            legacy_credential.secret_json,
            serde_json::json!({
                "Builtin": {
                    "Anthropic": {
                        "api_key": "sk-ant-legacy"
                    }
                }
            })
        );
        let moved_credential = credential_rows
            .iter()
            .find(|row| row.id == 202)
            .expect("moved credential exists");
        assert_eq!(moved_credential.provider_id, 2);

        let status_rows = credential_statuses::Entity::find()
            .filter(credential_statuses::Column::CredentialId.eq(101))
            .all(db)
            .await
            .expect("list statuses");
        assert_eq!(status_rows.len(), 1);
        let status = &status_rows[0];
        assert_eq!(status.channel, "anthropic");
        assert_eq!(status.health_kind, "dead");
        assert_eq!(status.last_error.as_deref(), Some("401"));

        let upstream = upstream_requests::Entity::find_by_id(401)
            .one(db)
            .await
            .expect("load upstream request")
            .expect("upstream request exists");
        assert_eq!(upstream.provider_id, Some(2));

        let usage = usages::Entity::find_by_id(501)
            .one(db)
            .await
            .expect("load usage")
            .expect("usage exists");
        assert_eq!(usage.provider_id, Some(2));
    }
}
