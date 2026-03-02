use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

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
