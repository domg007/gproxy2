use std::sync::Arc;

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use gproxy_server::AppState;
use gproxy_storage::Scope;

use crate::error::HttpError;

/// Hash a password with Argon2id and a random salt.
/// Returns a PHC-format string containing algorithm, salt, and hash.
pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("argon2 hash")
        .to_string()
}

/// Verify a password against a stored Argon2 PHC hash.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub user_id: i64,
    pub api_key: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, HttpError> {
    let storage = state.storage();
    let users = storage
        .list_users(&gproxy_storage::UserQuery {
            name: Scope::Eq(payload.username.clone()),
            ..Default::default()
        })
        .await?;

    let user = users
        .first()
        .ok_or_else(|| HttpError::unauthorized("invalid username or password"))?;

    if !verify_password(&payload.password, &user.password) {
        return Err(HttpError::unauthorized("invalid username or password"));
    }

    if !user.enabled {
        return Err(HttpError::forbidden("user is disabled"));
    }

    let keys = storage
        .list_user_keys(&gproxy_storage::UserKeyQuery {
            user_id: Scope::Eq(user.id),
            enabled: Scope::Eq(true),
            ..Default::default()
        })
        .await?;

    let key = keys
        .first()
        .ok_or_else(|| HttpError::internal("user has no API key"))?;

    Ok(Json(LoginResponse {
        user_id: user.id,
        api_key: key.api_key.clone(),
    }))
}
