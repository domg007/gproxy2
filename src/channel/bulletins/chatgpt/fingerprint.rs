//! Built-in TLS + HTTP/2 impersonation profile for the `chatgpt` channel.
//!
//! chatgpt.com is Cloudflare-fronted and rejects non-browser TLS, so the
//! channel impersonates Microsoft Edge 147 — matching the Edge-147 client-hints
//! header set in [`super::headers`] (`sec-ch-ua` v="147", Edge full-version
//! `147.0.3912.72`). Built from `wreq-util`'s captured Edge 147 emulation
//! (TLS + HTTP/2 + header order). Applied when no DB `tls_fingerprint`
//! overrides it. Native + `upstream-wreq` only.

use wreq::IntoEmulation;

pub(super) fn default_emulation() -> wreq::Emulation {
    wreq_util::Emulation::Edge147.into_emulation()
}
