use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Serialize;

use crate::auth::{
    self, cookie_token, request_secure, session_cookie, session_expiry, Credentials, MeBody,
};
use crate::error::ApiError;
use crate::state::AppState;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/setup/status", get(setup_status))
        .route("/setup", post(setup))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    name: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        name: "coduos",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct SetupStatus {
    needed: bool,
}

async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>, ApiError> {
    Ok(Json(SetupStatus {
        needed: state.db.user_count()? == 0,
    }))
}

async fn setup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<Credentials>,
) -> Result<(CookieJar, Json<MeBody>), ApiError> {
    if state.db.user_count()? != 0 {
        return Err(ApiError::Conflict("already set up".into()));
    }
    auth::validate_username(&body.username)?;
    auth::validate_password(&body.password)?;
    let hash = auth::hash_password(&body.password)?;
    let id = state.db.create_user(&body.username, &hash)?;
    let token = auth::new_token();
    state.db.insert_session(&token, id, &session_expiry())?;
    let jar = jar.add(session_cookie(&token, request_secure(&headers)));
    Ok((
        jar,
        Json(MeBody {
            username: body.username,
            setup_needed: false,
        }),
    ))
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<Credentials>,
) -> Result<(CookieJar, Json<MeBody>), ApiError> {
    if state.db.user_count()? == 0 {
        return Err(ApiError::SetupRequired);
    }
    let user = state
        .db
        .user_by_name(&body.username)?
        .ok_or(ApiError::Unauthorized)?;
    if !auth::verify_password(&body.password, &user.password_hash)? {
        return Err(ApiError::Unauthorized);
    }
    let token = auth::new_token();
    state
        .db
        .insert_session(&token, user.id, &session_expiry())?;
    let jar = jar.add(session_cookie(&token, request_secure(&headers)));
    Ok((
        jar,
        Json(MeBody {
            username: user.username,
            setup_needed: false,
        }),
    ))
}

async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Json<serde_json::Value>), ApiError> {
    if let Some(token) = cookie_token(&jar) {
        state.db.delete_session(&token)?;
    }
    Ok((jar.add(auth::clear_cookie()), Json(serde_json::json!({"ok": true}))))
}

async fn me(State(state): State<AppState>, jar: CookieJar) -> Result<Json<MeBody>, ApiError> {
    if state.db.user_count()? == 0 {
        return Ok(Json(MeBody {
            username: String::new(),
            setup_needed: true,
        }));
    }
    let user = current_user(&state, &jar).await?;
    Ok(Json(MeBody {
        username: user.username,
        setup_needed: false,
    }))
}
