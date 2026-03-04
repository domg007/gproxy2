use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::runtime::maybe_fix_deleted_exe_path;
use super::verify::{
    decode_hex_exact, parse_sha256_from_checksum_text, verify_downloaded_asset_sha256,
    verify_ed25519_detached_signature,
};

#[test]
fn maybe_fix_deleted_exe_path_strips_deleted_suffix_when_file_exists() {
    let base = unique_temp_path("gproxy-test-bin");
    fs::write(&base, b"bin").expect("write temp binary");
    let deleted = PathBuf::from(format!("{} (deleted)", base.display()));
    assert_eq!(maybe_fix_deleted_exe_path(deleted), base);
    let _ = fs::remove_file(base);
}

#[test]
fn maybe_fix_deleted_exe_path_keeps_original_when_candidate_missing() {
    let missing = unique_temp_path("gproxy-test-bin-missing");
    let deleted = PathBuf::from(format!("{} (deleted)", missing.display()));
    assert_eq!(maybe_fix_deleted_exe_path(deleted.clone()), deleted);
}

#[test]
fn maybe_fix_deleted_exe_path_keeps_original_for_non_deleted_name() {
    let regular = unique_temp_path("gproxy-test-bin-regular");
    assert_eq!(maybe_fix_deleted_exe_path(regular.clone()), regular);
}

#[test]
fn parse_sha256_from_file_supports_sha256sum_format() {
    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let text = format!("{hash}  gproxy-linux-x86_64.zip\n");
    let parsed = parse_sha256_from_checksum_text(&text, "gproxy-linux-x86_64.zip")
        .expect("hash should be parsed");
    assert_eq!(parsed, hash);
}

#[test]
fn parse_sha256_from_file_supports_single_hash_format() {
    let hash = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let text = format!("{hash}\n");
    let parsed = parse_sha256_from_checksum_text(&text, "gproxy-linux-x86_64.zip")
        .expect("hash should be parsed");
    assert_eq!(parsed, hash);
}

#[test]
fn verify_downloaded_asset_sha256_rejects_mismatch() {
    let wrong = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let err =
        verify_downloaded_asset_sha256(b"hello", "gproxy-linux-x86_64.zip", wrong).unwrap_err();
    assert!(err.contains("sha256_mismatch"));
}

#[test]
fn verify_ed25519_signature_accepts_rfc8032_vector() {
    let public_key = decode_hex_exact(
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        32,
    )
    .expect("valid rfc key");
    let signature_b64 =
        "5VZDAMNgrHKQhuLMgG6CioSHfx645dl02HPgZSJJAVVfuIIVkKM7rMYeOXAc+bRr0lv18FlbviRlUUFDjnoQCw==";
    verify_ed25519_detached_signature(b"", signature_b64.as_bytes(), public_key.as_slice())
        .expect("signature should verify");
}

#[test]
fn verify_ed25519_signature_rejects_modified_payload() {
    let public_key = decode_hex_exact(
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        32,
    )
    .expect("valid rfc key");
    let signature_b64 =
        "5VZDAMNgrHKQhuLMgG6CioSHfx645dl02HPgZSJJAVVfuIIVkKM7rMYeOXAc+bRr0lv18FlbviRlUUFDjnoQCw==";
    let err = verify_ed25519_detached_signature(
        b"tampered",
        signature_b64.as_bytes(),
        public_key.as_slice(),
    )
    .unwrap_err();
    assert!(err.contains("ed25519_verify_failed"));
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}
