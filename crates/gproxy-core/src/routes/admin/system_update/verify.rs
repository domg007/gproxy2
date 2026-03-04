use base64::Engine as _;
use ed25519_dalek::Verifier;
use sha2::{Digest, Sha256};

use super::runtime::download_bytes_with_redirects;
use super::types::{
    ResolvedReleaseAsset, UPDATE_SIGNING_KEY_ID, UPDATE_SIGNING_KEY_ID_DEFAULT,
    UPDATE_SIGNING_PUBLIC_KEY_B64,
};

pub(super) async fn resolve_release_asset_sha256(
    client: &wreq::Client,
    asset: &ResolvedReleaseAsset,
) -> Result<String, String> {
    let checksum_url = asset
        .sha256_url
        .as_deref()
        .ok_or_else(|| format!("sha256_source_missing:asset={}", asset.name))?;
    let checksum_bytes = download_bytes_with_redirects(client, checksum_url, 8).await?;
    let checksum_signature_url = asset
        .sha256_signature_url
        .as_deref()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{checksum_url}.sig"));
    let checksum_signature_bytes =
        download_bytes_with_redirects(client, checksum_signature_url.as_str(), 8).await?;
    verify_checksum_signature(
        checksum_bytes.as_slice(),
        checksum_signature_bytes.as_slice(),
        asset.signature_key_id.as_deref(),
    )
    .map_err(|err| format!("verify_checksum_signature:url={checksum_signature_url}:{err}"))?;

    let checksum_text = String::from_utf8(checksum_bytes)
        .map_err(|err| format!("sha256_file_not_utf8:url={checksum_url}:{err}"))?;
    let parsed = parse_sha256_from_checksum_text(checksum_text.as_str(), asset.name.as_str())
        .map_err(|err| format!("parse_sha256_from_file:url={checksum_url}:{err}"))?;
    if let Some(expected_sha256) = asset.expected_sha256.as_deref() {
        let expected = normalize_sha256_hex(expected_sha256).ok_or_else(|| {
            format!(
                "invalid_expected_sha256:asset={}:value={expected_sha256}",
                asset.name
            )
        })?;
        if expected != parsed {
            return Err(format!(
                "sha256_manifest_mismatch:asset={}:manifest={expected}:checksum_file={parsed}",
                asset.name
            ));
        }
    }
    Ok(parsed)
}

pub(super) fn normalize_sha256_hex(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(normalized)
}

pub(super) fn parse_sha256_from_checksum_text(
    checksum_text: &str,
    target_asset: &str,
) -> Result<String, String> {
    let mut single_hash: Option<String> = None;
    let mut single_hash_count = 0_u32;

    for line in checksum_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let Some(first) = parts.next() else {
            continue;
        };
        let Some(hash) = normalize_sha256_hex(first) else {
            continue;
        };

        let second = parts.next();
        if second.is_none() {
            single_hash_count = single_hash_count.saturating_add(1);
            single_hash = Some(hash.clone());
            continue;
        }

        let filename = second
            .map(|value| value.trim_start_matches('*').trim_matches('"'))
            .filter(|value| !value.is_empty());
        if let Some(filename) = filename
            && (filename == target_asset
                || filename.ends_with(&format!("/{target_asset}"))
                || filename.ends_with(&format!("\\{target_asset}")))
        {
            return Ok(hash);
        }
    }

    if single_hash_count == 1
        && let Some(hash) = single_hash
    {
        return Ok(hash);
    }

    Err(format!("sha256_not_found_for_target:{target_asset}"))
}

pub(super) fn verify_downloaded_asset_sha256(
    bytes: &[u8],
    asset_name: &str,
    expected_sha256: &str,
) -> Result<(), String> {
    let expected = normalize_sha256_hex(expected_sha256).ok_or_else(|| {
        format!("invalid_expected_sha256:asset={asset_name}:value={expected_sha256}")
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let actual = digest
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect::<String>();
    if actual != expected {
        return Err(format!(
            "sha256_mismatch:asset={asset_name}:expected={expected}:actual={actual}"
        ));
    }
    Ok(())
}

pub(super) fn verify_ed25519_detached_signature(
    message: &[u8],
    signature_bytes: &[u8],
    public_key_bytes: &[u8],
) -> Result<(), String> {
    let public_key: [u8; ed25519_dalek::PUBLIC_KEY_LENGTH] = public_key_bytes
        .try_into()
        .map_err(|_| format!("invalid_public_key_length:{}", public_key_bytes.len()))?;
    let signature = parse_ed25519_signature(signature_bytes)?;
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&public_key)
        .map_err(|err| format!("invalid_public_key: {err}"))?;
    verifying_key
        .verify(message, &signature)
        .map_err(|err| format!("ed25519_verify_failed: {err}"))
}

pub(super) fn decode_hex_exact(raw: &str, expected_len: usize) -> Option<Vec<u8>> {
    if raw.len() != expected_len * 2 {
        return None;
    }
    let mut out = Vec::with_capacity(expected_len);
    let bytes = raw.as_bytes();
    for idx in (0..bytes.len()).step_by(2) {
        let hi = bytes[idx];
        let lo = bytes[idx + 1];
        let hi = char::from(hi).to_digit(16)?;
        let lo = char::from(lo).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

pub(super) fn normalized_update_signing_key_id() -> String {
    let normalized = UPDATE_SIGNING_KEY_ID.trim();
    if normalized.is_empty() {
        return UPDATE_SIGNING_KEY_ID_DEFAULT.to_string();
    }
    normalized.to_string()
}

fn verify_checksum_signature(
    checksum_bytes: &[u8],
    signature_bytes: &[u8],
    signature_key_id: Option<&str>,
) -> Result<(), String> {
    let configured_key_id = normalized_update_signing_key_id();
    let resolved_key_id = signature_key_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(configured_key_id.as_str());
    if resolved_key_id != configured_key_id {
        return Err(format!(
            "untrusted_signature_key_id:configured={configured_key_id}:manifest={resolved_key_id}"
        ));
    }

    let public_key = load_update_signing_public_key()?;
    verify_ed25519_detached_signature(checksum_bytes, signature_bytes, public_key.as_slice())
        .map_err(|err| format!("checksum_signature_invalid:key_id={resolved_key_id}:{err}"))
}

fn parse_ed25519_signature(signature_bytes: &[u8]) -> Result<ed25519_dalek::Signature, String> {
    if signature_bytes.len() == ed25519_dalek::SIGNATURE_LENGTH {
        let bytes: [u8; ed25519_dalek::SIGNATURE_LENGTH] = signature_bytes
            .try_into()
            .map_err(|_| format!("invalid_signature_binary_length:{}", signature_bytes.len()))?;
        return Ok(ed25519_dalek::Signature::from_bytes(&bytes));
    }

    let signature_text = String::from_utf8(signature_bytes.to_vec())
        .map_err(|err| format!("signature_not_utf8: {err}"))?;
    let trimmed = signature_text.trim();
    if trimmed.is_empty() {
        return Err("signature_empty".to_string());
    }
    if let Some(decoded) = decode_hex_exact(trimmed, ed25519_dalek::SIGNATURE_LENGTH) {
        let bytes: [u8; ed25519_dalek::SIGNATURE_LENGTH] = decoded
            .try_into()
            .map_err(|_| format!("invalid_signature_hex_length:{}", trimmed.len()))?;
        return Ok(ed25519_dalek::Signature::from_bytes(&bytes));
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .map_err(|err| format!("signature_not_base64: {err}"))?;
    let decoded_len = decoded.len();
    let bytes: [u8; ed25519_dalek::SIGNATURE_LENGTH] = decoded
        .try_into()
        .map_err(|_| format!("invalid_signature_base64_length:{decoded_len}"))?;
    Ok(ed25519_dalek::Signature::from_bytes(&bytes))
}

fn load_update_signing_public_key() -> Result<Vec<u8>, String> {
    let raw = UPDATE_SIGNING_PUBLIC_KEY_B64.trim();
    if raw.is_empty() {
        return Err(
            "update_signature_public_key_missing:build_with_GPROXY_UPDATE_SIGN_PUBLIC_KEY_B64"
                .to_string(),
        );
    }
    if let Some(decoded) = decode_hex_exact(raw, ed25519_dalek::PUBLIC_KEY_LENGTH) {
        return Ok(decoded);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw))
        .map_err(|err| format!("update_signature_public_key_not_base64: {err}"))?;
    if decoded.len() != ed25519_dalek::PUBLIC_KEY_LENGTH {
        return Err(format!("invalid_public_key_length:{}", decoded.len()));
    }
    Ok(decoded)
}
