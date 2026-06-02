//! Liveness and version endpoints.

use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct Health {
    pub status: &'static str,
}

/// `GET /healthz` — liveness probe.
pub async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[derive(Serialize)]
pub struct Version {
    pub version: &'static str,
}

/// `GET /version` — report the running binary version.
pub async fn version() -> Json<Version> {
    Json(Version {
        version: env!("CARGO_PKG_VERSION"),
    })
}
