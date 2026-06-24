//! Local helper: verify a release `manifest.json` against a given ed25519 public
//! key, using the SAME [`gproxy::selfupdate::Manifest::signing_payload`] the
//! running binary uses at update time. This proves the signing tool
//! (`scripts/build-update-manifest.sh`) and the Rust verifier agree on the exact
//! canonical payload — the one thing a bash↔Rust split could silently get wrong.
//!
//! Usage:
//!   GPROXY_TEST_PUBKEY=<base64-32B> \
//!     cargo run --example verify-manifest -- path/to/manifest.json

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, VerifyingKey};
use gproxy::selfupdate::Manifest;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: verify-manifest <manifest.json> (GPROXY_TEST_PUBKEY=<base64 32B>)");
    let pubkey_b64 =
        std::env::var("GPROXY_TEST_PUBKEY").expect("set GPROXY_TEST_PUBKEY=<base64 32-byte key>");

    let json = std::fs::read_to_string(&path).expect("read manifest");
    let manifest = Manifest::parse(&json).expect("parse manifest");

    let raw = B64.decode(pubkey_b64.trim()).expect("pubkey base64");
    let arr: [u8; 32] = raw.as_slice().try_into().expect("pubkey must be 32 bytes");
    let key = VerifyingKey::from_bytes(&arr).expect("valid ed25519 key");

    let sig_bytes = B64
        .decode(manifest.signature.trim())
        .expect("signature base64");
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .expect("signature must be 64 bytes");
    let signature = Signature::from_bytes(&sig_arr);

    match key.verify_strict(&manifest.signing_payload(), &signature) {
        Ok(()) => println!(
            "OK: signature valid; channel={} version={} artifacts={}",
            manifest.channel,
            manifest.version,
            manifest.artifacts.len()
        ),
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    }
}
