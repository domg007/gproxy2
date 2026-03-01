use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use crate::AppState;

use super::{HttpError, authorize_admin};

pub(super) async fn query_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UsageQuery>,
) -> Result<Json<Vec<gproxy_storage::UsageQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(gproxy_admin::query_usages(&storage, query).await?))
}

pub(super) async fn summarize_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UsageQuery>,
) -> Result<Json<gproxy_storage::UsageSummary>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(gproxy_admin::summarize_usages(&storage, query).await?))
}
