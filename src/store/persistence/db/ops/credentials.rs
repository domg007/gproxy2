//! Credential ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{Credential, CredentialInput};

use super::super::entities::credential;

fn to_record(m: credential::Model) -> anyhow::Result<Credential> {
    Ok(Credential {
        id: m.id,
        provider_id: m.provider_id,
        name: m.name,
        kind: m.kind,
        secret_json: serde_json::from_str(&m.secret_json)?,
        weight: m.weight,
        rpm_limit: m.rpm_limit,
        tpm_limit: m.tpm_limit,
        proxy_url: m.proxy_url,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn list(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<Vec<Credential>> {
    credential::Entity::find()
        .filter(credential::Column::ProviderId.eq(provider_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect()
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<Credential>> {
    credential::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record)
        .transpose()
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: CredentialInput,
) -> anyhow::Result<Credential> {
    let now = super::now_secs();
    let secret = serde_json::to_string(&input.secret_json)?;

    let model = match input.id {
        Some(id) => {
            let existing = credential::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("credential not found: {id}"))?;
            let mut am: credential::ActiveModel = existing.into();
            am.provider_id = Set(input.provider_id);
            am.name = Set(input.name);
            am.kind = Set(input.kind);
            am.secret_json = Set(secret);
            am.weight = Set(input.weight);
            am.rpm_limit = Set(input.rpm_limit);
            am.tpm_limit = Set(input.tpm_limit);
            am.proxy_url = Set(input.proxy_url);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            credential::ActiveModel {
                id: NotSet,
                provider_id: Set(input.provider_id),
                name: Set(input.name),
                kind: Set(input.kind),
                secret_json: Set(secret),
                weight: Set(input.weight),
                rpm_limit: Set(input.rpm_limit),
                tpm_limit: Set(input.tpm_limit),
                proxy_url: Set(input.proxy_url),
                enabled: Set(input.enabled),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(conn)
            .await?
        }
    };

    to_record(model)
}

pub async fn delete(conn: &DatabaseConnection, id: i64) -> anyhow::Result<bool> {
    super::credential_statuses::delete_by_credential(conn, id).await?;
    let res = credential::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_provider(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<()> {
    credential::Entity::delete_many()
        .filter(credential::Column::ProviderId.eq(provider_id))
        .exec(conn)
        .await?;
    Ok(())
}
