// Optional cookie-authenticated live invalidations. Ordinary reads and writes
// work without subscribers; reads and the stale-review guard stay authoritative.
// Every notification is state-changed, optionally carrying a mutation UUID.
// Clients reread state, including displayed prompt validity, coalesce refreshes,
// and discard reads superseded by a notification. No mutation details are sent.
// Each subscription starts with the same notification; lag drops the backlog
// and sends another. There is no durable replay or Last-Event-ID contract.
// Subscribe before the initial read so concurrent changes are queued.
// Heartbeats do no DB work. This assumes one backend process and API mutations;
// external writers or multiple processes require shared event distribution.
// Clients resynchronise after reconnecting.
pub mod mutations;

use crate::{
    auth::AuthUser,
    error::{AppError, AppPath},
    passkey::AppState,
};
use axum::{
    Json,
    extract::State,
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, stream};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;
use utoipa::ToSchema;

// Correlation only, never identity or idempotency. Invalid UUIDs are ignored.
pub const MUTATION_HEADER: &str = "x-mutation-id";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Notification {
    StateChanged {
        #[serde(skip_serializing_if = "Option::is_none")]
        mutation_id: Option<String>,
    },
}
impl Notification {
    fn event(&self) -> Event {
        Event::default()
            .event("state-changed")
            .json_data(self)
            .expect("serializable notification")
    }
}

// Bounded per-user channels contain no answer data. Dropping the last receiver
// removes its channel; publishing without subscribers never makes a write fail.
#[derive(Clone, Default)]
pub struct EventHub(Arc<Mutex<HashMap<i64, broadcast::Sender<Notification>>>>);
impl EventHub {
    fn subscribe(&self, user_id: i64) -> Subscription {
        let mut users = self.0.lock().unwrap();
        let sender = users
            .entry(user_id)
            .or_insert_with(|| broadcast::channel(64).0);
        Subscription {
            receiver: Some(sender.subscribe()),
            hub: self.clone(),
            user_id,
        }
    }
    pub fn publish(&self, user_id: Option<i64>, headers: &HeaderMap) {
        // Bound input before parsing; only canonical UUIDs enter subscriber buffers.
        let mutation_id = headers
            .get(MUTATION_HEADER)
            .filter(|value| value.as_bytes().len() <= 45)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .map(|value| value.to_string());
        let message = Notification::StateChanged { mutation_id };
        let users = self.0.lock().unwrap();
        for (id, sender) in users.iter() {
            if user_id.is_none_or(|user| user == *id) {
                let _ = sender.send(message.clone());
            }
        }
    }
}
struct Subscription {
    receiver: Option<broadcast::Receiver<Notification>>,
    hub: EventHub,
    user_id: i64,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let mut users = self.hub.0.lock().unwrap();
        drop(self.receiver.take());
        if users
            .get(&self.user_id)
            .is_some_and(|sender| sender.receiver_count() == 0)
        {
            users.remove(&self.user_id);
        }
    }
}

#[utoipa::path(get, path = "/api/events", responses(
    (status = 200, description = "Named state-changed SSE notifications with JSON {\"type\":\"state-changed\"} and an optional mutation_id UUID for successful mutations; initial connection and lag omit it and also request a full reread. No replay or Last-Event-ID contract.", content_type = "text/event-stream", body = String),
    (status = 401, description = "Missing or expired authentication")
), security(("cookie_auth" = [])), tag = "events")]
pub async fn subscribe(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let token = crate::auth_cookie::token(&headers)?;
    let claims = crate::auth::decode_token(&token, &crate::credentials::jwt_secret())?;
    // Register before emitting the initial resync, closing the read/subscribe gap.
    let subscription = state.events.subscribe(auth.0);
    let expires = claims.exp.map(|exp| {
        tokio::time::Instant::now()
            + Duration::from_secs(exp.saturating_sub(chrono::Utc::now().timestamp()).max(0) as u64)
    });
    Ok(Sse::new(notification_stream(subscription, expires))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
fn notification_stream(
    subscription: Subscription,
    expires: Option<tokio::time::Instant>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(
        (subscription, true, expires),
        |(mut sub, initial, expires)| async move {
            if expires.is_some_and(|time| time <= tokio::time::Instant::now()) {
                return None;
            }
            if initial {
                return Some((
                    Ok(Notification::StateChanged { mutation_id: None }.event()),
                    (sub, false, expires),
                ));
            }
            let expiry = async {
                match expires {
                    Some(time) => tokio::time::sleep_until(time).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                _ = expiry => None,
                message = sub.receiver.as_mut().unwrap().recv() => {
                    let message = match message {
                        Ok(message) => message,
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // Drop the old backlog; the forthcoming read covers it.
                            sub.receiver = Some(sub.receiver.as_ref().unwrap().resubscribe());
                            Notification::StateChanged { mutation_id: None }
                        }
                        Err(broadcast::error::RecvError::Closed) => return None,
                    };
                    Some((Ok(message.event()), (sub, false, expires)))
                }
            }
        },
    )
}

// Prompt validity without answer fields. This does not say whether the card
// ranks first or would be selected under current queue preferences. Both
// revisions also travel with next-card responses; content revisions detect
// edits missed while disconnected.
#[derive(Serialize, ToSchema)]
pub struct CardState {
    pub exists: bool,
    pub suppressed: bool,
    pub review_revision: i64,
    pub content_revision: i64,
}
#[utoipa::path(get, path = "/api/cards/{card_id}/state", params(("card_id" = i64, Path)), responses(
    (status = 200, description = "Current prompt validity, without answer fields", body = CardState),
    (status = 401, description = "Missing authentication")
), security(("cookie_auth" = [])), tag = "cards")]
pub async fn card_state(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
    AppPath(card_id): AppPath<i64>,
) -> Result<Json<CardState>, AppError> {
    let row = sqlx::query(
        "SELECT c.content_revision, COALESCE(cs.review_revision, 0) AS review_revision,
        COALESCE(f.suppressed, 0) AS suppressed FROM cards c
        LEFT JOIN cards_states cs ON cs.card_id = c.id AND cs.user_id = ?
        LEFT JOIN users_card_flags f ON f.card_id = c.id AND f.user_id = ? WHERE c.id = ?",
    )
    .bind(auth.0)
    .bind(auth.0)
    .bind(card_id)
    .fetch_optional(&pool)
    .await?;
    Ok(Json(match row {
        Some(row) => CardState {
            exists: true,
            suppressed: row.get("suppressed"),
            review_revision: row.get("review_revision"),
            content_revision: row.get("content_revision"),
        },
        None => CardState {
            exists: false,
            suppressed: false,
            review_revision: 0,
            content_revision: 0,
        },
    }))
}

#[cfg(test)]
#[path = "tests/events.rs"]
mod tests;
