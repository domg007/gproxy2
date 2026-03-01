use axum::http::HeaderMap;

use crate::AppState;

use super::{ADMIN_USER_ID, HttpError, X_API_KEY};

pub(super) fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

pub(super) fn authorize_admin(headers: &HeaderMap, state: &AppState) -> Result<(), HttpError> {
    let api_key =
        gproxy_admin::extract_api_key(header_value(headers, X_API_KEY)).map_err(HttpError::from)?;
    let Some(key) = state.authenticate_api_key_in_memory(api_key) else {
        return Err(HttpError::from(gproxy_admin::AdminApiError::Unauthorized));
    };
    if key.user_id != ADMIN_USER_ID {
        return Err(HttpError::from(gproxy_admin::AdminApiError::Forbidden));
    }
    Ok(())
}
