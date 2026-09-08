use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use std::convert::Infallible;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::StreamExt;

use crate::error::ApiError;
use crate::state::AppState;
use crate::stats::SystemSummary;

use super::current_user;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/summary", get(summary))
        .route("/system/summary/stream", get(stream))
}

async fn summary(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<SystemSummary>, ApiError> {
    current_user(&state, &jar).await?;
    Ok(Json(state.summary_rx.borrow().clone()))
}

async fn stream(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    current_user(&state, &jar).await?;
    let rx = state.summary_tx.subscribe();
    let s = WatchStream::new(rx).map(|sum| match Event::default().json_data(sum) {
        Ok(ev) => Ok(ev),
        Err(_) => Ok(Event::default().data("{}")),
    });
    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
}
