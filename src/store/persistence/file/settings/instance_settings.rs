//! File-backend instance settings ops over `instance_settings.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{InstanceSettings, InstanceSettingsInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("instance_settings.json")
}

pub(crate) async fn list(root: &Path) -> anyhow::Result<Vec<InstanceSettings>> {
    Ok(table::load::<InstanceSettings>(&path(root)).await?.rows)
}

pub(crate) async fn get(
    root: &Path,
    instance_name: &str,
) -> anyhow::Result<Option<InstanceSettings>> {
    Ok(table::load::<InstanceSettings>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|s| s.instance_name == instance_name))
}

pub(crate) async fn upsert(
    root: &Path,
    input: InstanceSettingsInput,
) -> anyhow::Result<InstanceSettings> {
    let file = path(root);
    let mut t = table::load::<InstanceSettings>(&file).await?;
    let now = now_secs();

    if let Some(existing) = t
        .rows
        .iter()
        .find(|s| s.instance_name == input.instance_name)
        && Some(existing.id) != input.id
    {
        anyhow::bail!("instance name already exists: {}", input.instance_name);
    }

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| anyhow::anyhow!("instance settings not found: {id}"))?;
            row.instance_name = input.instance_name;
            row.proxy = input.proxy;
            row.spoof_emulation = input.spoof_emulation;
            row.enable_usage = input.enable_usage;
            row.enable_upstream_log = input.enable_upstream_log;
            row.enable_upstream_log_body = input.enable_upstream_log_body;
            row.enable_downstream_log = input.enable_downstream_log;
            row.enable_downstream_log_body = input.enable_downstream_log_body;
            row.disable_log_redaction = input.disable_log_redaction;
            row.update_channel = input.update_channel;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let settings = InstanceSettings {
                id,
                instance_name: input.instance_name,
                proxy: input.proxy,
                spoof_emulation: input.spoof_emulation,
                enable_usage: input.enable_usage,
                enable_upstream_log: input.enable_upstream_log,
                enable_upstream_log_body: input.enable_upstream_log_body,
                enable_downstream_log: input.enable_downstream_log,
                enable_downstream_log_body: input.enable_downstream_log_body,
                disable_log_redaction: input.disable_log_redaction,
                update_channel: input.update_channel,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(settings.clone());
            settings
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}
