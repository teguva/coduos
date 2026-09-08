use axum::Router;
use axum_extra::extract::CookieJar;

use crate::auth;
use crate::db::UserRow;
use crate::error::ApiError;
use crate::state::AppState;

mod apps;
mod auth_routes;
mod files;
mod settings;
mod system;
mod units;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(auth_routes::router())
        .merge(system::router())
        .merge(apps::router())
        .merge(files::router())
        .merge(units::router())
        .merge(settings::router())
}

pub async fn current_user(state: &AppState, jar: &CookieJar) -> Result<UserRow, ApiError> {
    auth::require_user(state, auth::cookie_token(jar).as_deref()).await
}
