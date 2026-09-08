use axum::http::HeaderMap;
use axum_extra::extract::CookieJar;
use chrono::{TimeDelta, Utc};
use cookie::{Cookie, SameSite};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::db::UserRow;
use crate::error::ApiError;
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "coduos_session";
const SESSION_DAYS: i64 = 7;

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    use argon2::password_hash::SaltString;
    use argon2::{Argon2, PasswordHasher};
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(ApiError::internal)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, ApiError> {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let parsed = PasswordHash::new(hash).map_err(ApiError::internal)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn session_cookie(token: &str, secure: bool) -> Cookie<'static> {
    let mut cookie = Cookie::build((SESSION_COOKIE, token.to_string()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::days(SESSION_DAYS))
        .build();
    if secure {
        cookie.set_secure(true);
    }
    cookie
}

pub fn clear_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .max_age(cookie::time::Duration::seconds(0))
        .build()
}

pub fn session_expiry() -> String {
    (Utc::now() + TimeDelta::days(SESSION_DAYS)).to_rfc3339()
}

pub async fn require_user(state: &AppState, token: Option<&str>) -> Result<UserRow, ApiError> {
    if state.db.user_count()? == 0 {
        return Err(ApiError::SetupRequired);
    }
    let Some(token) = token else {
        return Err(ApiError::Unauthorized);
    };
    state.db.session_user(token)?.ok_or(ApiError::Unauthorized)
}

pub fn validate_username(username: &str) -> Result<(), ApiError> {
    if username.len() < 3 || username.len() > 32 {
        return Err(ApiError::BadRequest(
            "username must be 3–32 characters".into(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ApiError::BadRequest(
            "username may contain letters, numbers, _ and -".into(),
        ));
    }
    Ok(())
}

pub fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.len() < 8 {
        return Err(ApiError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct MeBody {
    pub username: String,
    pub setup_needed: bool,
}

pub fn request_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

pub fn cookie_token(jar: &CookieJar) -> Option<String> {
    jar.get(SESSION_COOKIE).map(|c| c.value().to_string())
}
