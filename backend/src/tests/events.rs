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
    hub.publish(Some(1), None, &HeaderMap::new());
    assert!(matches!(
        a.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::StateChanged { .. }
    ));
    assert!(matches!(
        a2.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::StateChanged { .. }
    ));
    assert!(matches!(
        b.receiver.as_mut().unwrap().try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    hub.publish(None, Some(1), &HeaderMap::new());
    assert!(matches!(
        b.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::CardMutated { card_id: 1, .. }
    ));
    drop(a);
    assert_eq!(hub.0.lock().unwrap().len(), 2);
    drop(a2);
    drop(b);
    assert!(hub.0.lock().unwrap().is_empty());
    hub.publish(Some(1), None, &HeaderMap::new());
    assert!(hub.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn subscribe_before_initial_sync_and_lag_requires_another_sync() {
    let hub = EventHub::default();
    let sub = hub.subscribe(1);
    hub.publish(Some(1), None, &HeaderMap::new());
    let mut body = Sse::new(notification_stream(sub, None))
        .into_response()
        .into_body()
        .into_data_stream();
    let resync = "event: resync-required\ndata: {\"type\":\"resync-required\"}\n\n";
    let first = body.next().await.unwrap().unwrap();
    let expected = "event: state-changed\ndata: {\"type\":\"state-changed\"}\n\n";
    assert_eq!(std::str::from_utf8(&first).unwrap(), resync);
    let second = body.next().await.unwrap().unwrap();
    assert_eq!(std::str::from_utf8(&second).unwrap(), expected);
    for _ in 0..100 {
        hub.publish(Some(1), None, &HeaderMap::new());
    }
    let lag = body.next().await.unwrap().unwrap();
    assert_eq!(std::str::from_utf8(&lag).unwrap(), resync);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), body.next())
            .await
            .is_err()
    );
    hub.publish(Some(1), None, &HeaderMap::new());
    let next = body.next().await.unwrap().unwrap();
    assert_eq!(std::str::from_utf8(&next).unwrap(), expected);
    drop(body);
    assert!(hub.0.lock().unwrap().is_empty());
    let mut reconnected = Sse::new(notification_stream(hub.subscribe(1), None))
        .into_response()
        .into_body()
        .into_data_stream();
    let initial = reconnected.next().await.unwrap().unwrap();
    assert_eq!(std::str::from_utf8(&initial).unwrap(), resync);
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
async fn review_notifies_after_commit_and_stale_writes_emit_nothing() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    let state = AppState::new(pool.clone());
    let mut subscription = state.events.subscribe(user);
    let answer: String = sqlx::query_scalar("SELECT tg.form FROM sentences s JOIN targets tg ON tg.sentence_id = s.id WHERE s.card_id = 1")
        .fetch_one(&pool).await.unwrap();
    let _ = mutations::check_answer(
        State(state.clone()),
        AppPath(1),
        AuthUser(user),
        HeaderMap::new(),
        AppJson(crate::cards::CheckRequest {
            answer: answer.clone(),
            attempt_token: crate::cards::issue_attempt_token(user, 1, 1, 0, 0).unwrap(),
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
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({"type": "card-mutated", "card_id": 1})
    );
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
                attempt_token: crate::cards::issue_attempt_token(user, 1, 1, 0, 0).unwrap(),
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
        sub.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::CardMutated { card_id: 1, .. }
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
        sub.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::CardMutated { card_id: 1, .. }
    ));
    let payload = serde_json::from_value(serde_json::json!({"daily_new_card_limit": 25})).unwrap();
    let _ = mutations::update_settings(
        State(state.clone()),
        AuthUser(user),
        HeaderMap::new(),
        AppJson(payload),
    )
    .await
    .unwrap();
    assert!(matches!(
        sub.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::StateChanged { .. }
    ));
    let payload = serde_json::from_value(serde_json::json!({"daily_new_card_limit": -1})).unwrap();
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
        sub.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::ResyncRequired { .. }
    ));
    assert!(sub.receiver.as_mut().unwrap().try_recv().is_err());
}

#[tokio::test]
async fn shared_edit_advances_content_revision_and_notifies_other_users() {
    let pool = test_pool().await;
    let admin = crate::test_support::test_admin_user(&pool).await;
    let state = AppState::new(pool.clone());
    let mut sub = state.events.subscribe(123);
    let payload =
        serde_json::from_value(serde_json::json!({"sentence_id": 1, "hanja": "test"})).unwrap();
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
        sub.receiver.as_mut().unwrap().recv().await.unwrap(),
        Notification::CardMutated { card_id: 1, .. }
    ));
    let card = card_state(State(pool), AuthUser(admin), AppPath(1))
        .await
        .unwrap()
        .0;
    assert_eq!(card.content_revision, 1);
    let json = serde_json::to_value(card).unwrap();
    assert!(json.get("target").is_none());
}
