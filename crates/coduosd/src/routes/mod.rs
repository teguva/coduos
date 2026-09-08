use axum::Router;
use axum_extra::extract::CookieJar;

use crate::auth;
use crate::db::UserRow;
use crate::error::ApiError;
use crate::state::AppState;

mod apps;
mod auth_routes;
mod files;
mod icons;
mod proxy;
mod settings;
mod storage;
mod system;
mod units;
mod vpn;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(auth_routes::router())
        .merge(system::router())
        .merge(apps::router())
        .merge(files::router())
        .merge(icons::router())
        .merge(units::router())
        .merge(settings::router())
        .merge(storage::router())
        .merge(vpn::router())
        .merge(proxy::router())
}

pub async fn current_user(state: &AppState, jar: &CookieJar) -> Result<UserRow, ApiError> {
    auth::require_user(state, auth::cookie_token(jar).as_deref()).await
}
