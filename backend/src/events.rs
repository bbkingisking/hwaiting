//! Optional cookie-authenticated live invalidations. Ordinary reads and writes
//! work without subscribers; reads and the stale-review guard stay authoritative.
//!
//! Events describe committed changes, not UI actions. Concurrent mutations may
//! publish out of commit order: compare review revisions and reread state for
//! broader changes. Clients should coalesce refreshes and discard reads superseded
//! by a notification. Each JSON `type` matches its named SSE event.
//!
//! Each subscription starts with `resync-required`; there is no durable replay or
//! `Last-Event-ID` contract. Subscribe before the initial resync so changes during
//! the client's read are queued. Lag also requires resync, including validation
//! of the displayed prompt, not just counts/deadlines. Heartbeats do no DB work.
//!
//! This assumes one backend process and mutations through its API. Multiple
//! processes or external database writers require shared event distribution.
//! A process crash disconnects clients, which resynchronise after reconnecting.
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

// Optional UUID echoed for operation correlation, not identity or idempotency.
// Invalid UUIDs are ignored. Clients register it before sending the mutation.
pub const MUTATION_HEADER: &str = "x-mutation-id";

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Change {
    // Also invalidates queue/stats; do not emit an additional QueueChanged.
    CardReviewed { card_id: i64, review_revision: i64 },
    // User-specific suppression or restoration.
    QueueChanged { card_id: i64, suppressed: bool },
    // Settings/FSRS parameters changed; refetch settings and any affected queue.
    SettingsChanged { queue_affected: bool },
    // Shared content changed; broadcast to every connected account.
    CardUpdated { card_id: i64 },
    // Import may replace settings, history, flags, and review state.
    StateReset,
    // Reread current state, including the validity of the displayed prompt.
    ResyncRequired,
}
impl Change {
    fn name(&self) -> &'static str {
        match self {
            Self::CardReviewed { .. } => "card-reviewed",
            Self::QueueChanged { .. } => "queue-changed",
            Self::SettingsChanged { .. } => "settings-changed",
            Self::CardUpdated { .. } => "card-updated",
            Self::StateReset => "state-reset",
            Self::ResyncRequired => "resync-required",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Notification {
    #[serde(flatten)]
    pub change: Change,
    pub mutation_id: Option<String>,
}
impl Notification {
    fn resync() -> Self {
        Self {
            change: Change::ResyncRequired,
            mutation_id: None,
        }
    }
    fn event(&self) -> Event {
        Event::default()
            .event(self.change.name())
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
    pub fn publish(&self, user_id: Option<i64>, change: Change, headers: &HeaderMap) {
        // Correlation only, never identity/authorization. Bounded and parsed so
        // arbitrary header contents cannot bloat every subscriber's buffer.
        let mutation_id = headers
            .get(MUTATION_HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| uuid::Uuid::parse_str(v).ok())
            .map(|v| v.to_string());
        let message = Notification {
            change,
            mutation_id,
        };
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
    (status = 200, description = "Named SSE invalidations; first event requests resynchronisation. No replay or Last-Event-ID contract.", content_type = "text/event-stream", body = String),
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
                return Some((Ok(Notification::resync().event()), (sub, false, expires)));
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
                            Notification::resync()
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
mod tests {
    use super::*;
    use crate::{
        error::AppJson,
        test_support::{test_pool, test_user},
    };
    use axum::response::IntoResponse;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn user_isolation_global_edits_and_subscription_cleanup() {
        let hub = EventHub::default();
        let mut a = hub.subscribe(1);
        let mut b = hub.subscribe(2);
        let mut a2 = hub.subscribe(1);
        hub.publish(Some(1), Change::StateReset, &HeaderMap::new());
        assert!(matches!(
            a.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::StateReset
        ));
        assert!(matches!(
            a2.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::StateReset
        ));
        assert!(matches!(
            b.receiver.as_mut().unwrap().try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        hub.publish(None, Change::CardUpdated { card_id: 4 }, &HeaderMap::new());
        assert!(matches!(
            b.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::CardUpdated { card_id: 4 }
        ));
        drop(a);
        assert_eq!(hub.0.lock().unwrap().len(), 2);
        drop(a2);
        drop(b);
        assert!(hub.0.lock().unwrap().is_empty());
        hub.publish(Some(1), Change::StateReset, &HeaderMap::new());
        assert!(hub.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn subscribe_before_initial_sync_and_lag_requires_another_sync() {
        let hub = EventHub::default();
        let sub = hub.subscribe(1);
        hub.publish(
            Some(1),
            Change::CardReviewed {
                card_id: 42,
                review_revision: 8,
            },
            &HeaderMap::new(),
        );
        let mut body = Sse::new(notification_stream(sub, None))
            .into_response()
            .into_body()
            .into_data_stream();
        let first = body.next().await.unwrap().unwrap();
        assert!(
            std::str::from_utf8(&first)
                .unwrap()
                .contains("event: resync-required")
        );
        let second = body.next().await.unwrap().unwrap();
        assert!(
            std::str::from_utf8(&second)
                .unwrap()
                .contains("event: card-reviewed")
        );
        for _ in 0..100 {
            hub.publish(Some(1), Change::StateReset, &HeaderMap::new());
        }
        let lag = body.next().await.unwrap().unwrap();
        assert!(
            std::str::from_utf8(&lag)
                .unwrap()
                .contains("event: resync-required")
        );
        drop(body);
        assert!(hub.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expiry_closes_idle_stream_and_releases_subscription() {
        let hub = EventHub::default();
        let mut body = Sse::new(notification_stream(
            hub.subscribe(1),
            Some(tokio::time::Instant::now() + Duration::from_millis(20)),
        ))
        .into_response()
        .into_body()
        .into_data_stream();
        assert!(body.next().await.is_some());
        assert!(
            tokio::time::timeout(Duration::from_secs(1), body.next())
                .await
                .unwrap()
                .is_none()
        );
        assert!(hub.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn review_event_matches_committed_revision_and_stale_writes_emit_nothing() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        let state = AppState::new(pool.clone());
        let mut subscription = state.events.subscribe(user);
        let mutation = uuid::Uuid::new_v4().to_string();
        let mut headers = HeaderMap::new();
        headers.insert(MUTATION_HEADER, mutation.parse().unwrap());
        let answer: String = sqlx::query_scalar("SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = 1")
            .fetch_one(&pool).await.unwrap();
        let _ = mutations::check_answer(
            State(state.clone()),
            AppPath(1),
            AuthUser(user),
            headers,
            AppJson(crate::cards::CheckRequest {
                answer: answer.clone(),
                review_revision: 0,
            }),
        )
        .await
        .unwrap();
        let event = subscription
            .receiver
            .as_mut()
            .unwrap()
            .recv()
            .await
            .unwrap();
        assert_eq!(event.mutation_id.as_deref(), Some(mutation.as_str()));
        assert!(matches!(
            event.change,
            Change::CardReviewed {
                card_id: 1,
                review_revision: 1
            }
        ));
        let Json(card) = card_state(State(pool), AuthUser(user), AppPath(1))
            .await
            .unwrap();
        assert_eq!(card.review_revision, 1);
        assert!(
            mutations::check_answer(
                State(state),
                AppPath(1),
                AuthUser(user),
                HeaderMap::new(),
                AppJson(crate::cards::CheckRequest {
                    answer,
                    review_revision: 0
                })
            )
            .await
            .is_err()
        );
        assert!(matches!(
            subscription.receiver.as_mut().unwrap().try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn suppression_settings_and_import_emit_single_scoped_events() {
        let pool = test_pool().await;
        let user = test_user(&pool).await;
        let state = AppState::new(pool.clone());
        let mut sub = state.events.subscribe(user);
        let _ = mutations::suppress_card(
            State(state.clone()),
            AppPath(1),
            AuthUser(user),
            HeaderMap::new(),
        )
        .await
        .unwrap();
        assert!(matches!(
            sub.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::QueueChanged {
                card_id: 1,
                suppressed: true
            }
        ));
        assert!(
            card_state(State(pool.clone()), AuthUser(user), AppPath(1))
                .await
                .unwrap()
                .0
                .suppressed
        );
        let _ = mutations::unsuppress_card(
            State(state.clone()),
            AppPath(1),
            AuthUser(user),
            HeaderMap::new(),
        )
        .await
        .unwrap();
        assert!(matches!(
            sub.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::QueueChanged {
                suppressed: false,
                ..
            }
        ));
        let payload =
            serde_json::from_value(serde_json::json!({"daily_new_card_limit": 25})).unwrap();
        let _ = mutations::update_settings(
            State(state.clone()),
            AuthUser(user),
            HeaderMap::new(),
            AppJson(payload),
        )
        .await
        .unwrap();
        assert!(matches!(
            sub.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::SettingsChanged {
                queue_affected: true
            }
        ));
        let payload =
            serde_json::from_value(serde_json::json!({"daily_new_card_limit": -1})).unwrap();
        assert!(
            mutations::update_settings(
                State(state.clone()),
                AuthUser(user),
                HeaderMap::new(),
                AppJson(payload)
            )
            .await
            .is_err()
        );
        assert!(sub.receiver.as_mut().unwrap().try_recv().is_err());
        let data = crate::export_import::export_data(State(pool), AuthUser(user))
            .await
            .unwrap()
            .0;
        let payload =
            serde_json::from_value(serde_json::json!({"data": data, "overwrite": true})).unwrap();
        let _ = mutations::import_data(
            State(state),
            AuthUser(user),
            HeaderMap::new(),
            AppJson(payload),
        )
        .await
        .unwrap();
        assert!(matches!(
            sub.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::StateReset
        ));
        assert!(sub.receiver.as_mut().unwrap().try_recv().is_err());
    }

    #[tokio::test]
    async fn shared_edit_advances_content_revision_and_notifies_other_users() {
        let pool = test_pool().await;
        let admin = crate::test_support::test_admin_user(&pool).await;
        let state = AppState::new(pool.clone());
        let mut sub = state.events.subscribe(123);
        let payload = serde_json::from_value(serde_json::json!({"hanja": "test"})).unwrap();
        let _ = mutations::edit_card(
            crate::auth::AdminUser(admin),
            State(state),
            AppPath(1),
            HeaderMap::new(),
            AppJson(payload),
        )
        .await
        .unwrap();
        assert!(matches!(
            sub.receiver.as_mut().unwrap().recv().await.unwrap().change,
            Change::CardUpdated { card_id: 1 }
        ));
        let card = card_state(State(pool), AuthUser(admin), AppPath(1))
            .await
            .unwrap()
            .0;
        assert_eq!(card.content_revision, 1);
        let json = serde_json::to_value(card).unwrap();
        assert!(json.get("target").is_none());
    }
}
