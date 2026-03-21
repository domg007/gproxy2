use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

use crate::AppState;

use super::{HttpError, authorize_admin};

pub(super) async fn query_upstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UpstreamRequestQuery>,
) -> Result<Json<Vec<gproxy_storage::UpstreamRequestQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_upstream_requests(&storage, query).await?,
    ))
}

pub(super) async fn query_downstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::DownstreamRequestQuery>,
) -> Result<Json<Vec<gproxy_storage::DownstreamRequestQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_downstream_requests(&storage, query).await?,
    ))
}

pub(super) async fn count_upstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UpstreamRequestQuery>,
) -> Result<Json<gproxy_storage::RequestQueryCount>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::count_upstream_requests(&storage, query).await?,
    ))
}

pub(super) async fn count_downstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::DownstreamRequestQuery>,
) -> Result<Json<gproxy_storage::RequestQueryCount>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::count_downstream_requests(&storage, query).await?,
    ))
}

pub(super) async fn clear_upstream_request_payloads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ClearRequestPayload>,
) -> Result<Json<ClearRequestAck>, HttpError> {
    authorize_admin(&headers, &state)?;
    let ids = normalize_trace_ids(payload.trace_ids);
    if !payload.all && ids.is_empty() {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "trace_ids must be non-empty when all=false",
        ));
    }

    let storage = state.load_storage();
    let cleared = gproxy_admin::clear_upstream_request_payloads(
        &storage,
        if payload.all {
            None
        } else {
            Some(ids.as_slice())
        },
    )
    .await?;
    Ok(Json(ClearRequestAck { ok: true, cleared }))
}

pub(super) async fn clear_downstream_request_payloads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ClearRequestPayload>,
) -> Result<Json<ClearRequestAck>, HttpError> {
    authorize_admin(&headers, &state)?;
    let ids = normalize_trace_ids(payload.trace_ids);
    if !payload.all && ids.is_empty() {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "trace_ids must be non-empty when all=false",
        ));
    }

    let storage = state.load_storage();
    let cleared = gproxy_admin::clear_downstream_request_payloads(
        &storage,
        if payload.all {
            None
        } else {
            Some(ids.as_slice())
        },
    )
    .await?;
    Ok(Json(ClearRequestAck { ok: true, cleared }))
}

pub(super) async fn delete_upstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ClearRequestPayload>,
) -> Result<Json<ClearRequestAck>, HttpError> {
    authorize_admin(&headers, &state)?;
    let ids = normalize_trace_ids(payload.trace_ids);
    if !payload.all && ids.is_empty() {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "trace_ids must be non-empty when all=false",
        ));
    }

    let storage = state.load_storage();
    let cleared = gproxy_admin::delete_upstream_requests(
        &storage,
        if payload.all {
            None
        } else {
            Some(ids.as_slice())
        },
    )
    .await?;
    Ok(Json(ClearRequestAck { ok: true, cleared }))
}

pub(super) async fn delete_downstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ClearRequestPayload>,
) -> Result<Json<ClearRequestAck>, HttpError> {
    authorize_admin(&headers, &state)?;
    let ids = normalize_trace_ids(payload.trace_ids);
    if !payload.all && ids.is_empty() {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "trace_ids must be non-empty when all=false",
        ));
    }

    let storage = state.load_storage();
    let cleared = gproxy_admin::delete_downstream_requests(
        &storage,
        if payload.all {
            None
        } else {
            Some(ids.as_slice())
        },
    )
    .await?;
    Ok(Json(ClearRequestAck { ok: true, cleared }))
}

fn normalize_trace_ids(raw: Vec<i64>) -> Vec<i64> {
    let mut ids: Vec<i64> = raw.into_iter().filter(|id| *id > 0).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ClearRequestPayload {
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub trace_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct ClearRequestAck {
    pub ok: bool,
    pub cleared: u64,
}
