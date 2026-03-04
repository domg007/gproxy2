use super::types::{UPDATE_CHANNEL_RELEASES, UPDATE_CHANNEL_STAGING};

pub(super) fn normalize_update_channel(value: Option<&str>) -> String {
    let normalized = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    match normalized.as_deref() {
        Some(UPDATE_CHANNEL_STAGING) => UPDATE_CHANNEL_STAGING.to_string(),
        Some("release") | Some(UPDATE_CHANNEL_RELEASES) | Some("stable") => {
            UPDATE_CHANNEL_RELEASES.to_string()
        }
        _ => build_update_channel(),
    }
}

pub(super) fn is_semver_update_available(current_version: &str, latest_release_tag: &str) -> bool {
    let current = parse_semver_triplet(current_version);
    let latest = parse_semver_triplet(latest_release_tag);
    match (current, latest) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

fn build_update_channel() -> String {
    let channel = option_env!("GPROXY_UPDATE_CHANNEL")
        .unwrap_or("stable")
        .trim()
        .to_ascii_lowercase();
    match channel.as_str() {
        UPDATE_CHANNEL_STAGING => UPDATE_CHANNEL_STAGING.to_string(),
        _ => UPDATE_CHANNEL_RELEASES.to_string(),
    }
}

fn parse_semver_triplet(input: &str) -> Option<(u64, u64, u64)> {
    let trimmed = input.trim();
    let no_v = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let core = no_v.split('-').next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    Some((major, minor, patch))
}
