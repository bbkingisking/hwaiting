use super::*;
use crate::test_support::{test_pool, test_user};

#[tokio::test]
async fn study_timezone_initializes_once_and_can_be_changed_explicitly() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    assert_eq!(
        get_settings(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .core
            .study_timezone,
        None
    );
    for (patch, expected) in [
        (
            serde_json::json!({"initial_study_timezone": "Asia/Seoul"}),
            "Asia/Seoul",
        ),
        (
            serde_json::json!({"initial_study_timezone": "America/New_York"}),
            "Asia/Seoul",
        ),
        (serde_json::json!({"day_boundary_hour": 2}), "Asia/Seoul"),
        (
            serde_json::json!({"study_timezone": "Europe/London"}),
            "Europe/London",
        ),
        (
            serde_json::json!({"initial_study_timezone": "Asia/Tokyo"}),
            "Europe/London",
        ),
    ] {
        let _ = update_settings(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(serde_json::from_value(patch).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(
            get_settings(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .core
                .study_timezone
                .as_deref(),
            Some(expected)
        );
    }
    for field in ["study_timezone", "initial_study_timezone"] {
        for value in ["", "Mars/Olympus", "+09:00"] {
            let result = update_settings(
                State(pool.clone()),
                AuthUser(user_id),
                AppJson(serde_json::from_value(serde_json::json!({field: value})).unwrap()),
            )
            .await;
            assert!(matches!(result, Err(AppError::BadRequest(_))));
        }
    }
}

#[tokio::test]
async fn hanja_hint_preference_defaults_on_and_partial_updates_preserve_it() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let other_user_id = test_user(&pool).await;

    let initial = get_settings(State(pool.clone()), AuthUser(user_id))
        .await
        .unwrap()
        .0;
    assert!(initial.core.show_hanja_hints);

    for patch in [
        serde_json::json!({ "show_hanja_hints": false }),
        serde_json::json!({ "history_colored_dots": true }),
        serde_json::json!({}),
    ] {
        let response = update_settings(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(serde_json::from_value(patch).unwrap()),
        )
        .await
        .unwrap();
        assert!(!response.core.show_hanja_hints);
        let saved = get_settings(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0;
        assert!(!saved.core.show_hanja_hints);
        assert_eq!(
            saved.core.daily_new_card_limit,
            initial.core.daily_new_card_limit
        );
    }

    let other = get_settings(State(pool.clone()), AuthUser(other_user_id))
        .await
        .unwrap()
        .0;
    assert!(other.core.show_hanja_hints);

    let response = update_settings(
        State(pool.clone()),
        AuthUser(user_id),
        AppJson(serde_json::from_value(serde_json::json!({ "show_hanja_hints": true })).unwrap()),
    )
    .await
    .unwrap();
    assert!(response.core.show_hanja_hints);
    let restored = get_settings(State(pool), AuthUser(user_id))
        .await
        .unwrap()
        .0;
    assert!(restored.core.show_hanja_hints);
    assert!(restored.core.history_colored_dots);
}

#[tokio::test]
async fn translation_language_is_explicit_persisted_and_validated() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    assert_eq!(
        get_settings(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .core
            .translation_language,
        None
    );
    for code in ["eng", "kor"] {
        let payload =
            serde_json::from_value(serde_json::json!({"translation_language": code})).unwrap();
        let _ = update_settings(State(pool.clone()), AuthUser(user_id), AppJson(payload))
            .await
            .unwrap();
        assert_eq!(
            get_settings(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .core
                .translation_language
                .as_deref(),
            Some(code)
        );
    }
    for code in ["en", "ENG", "", "zzz"] {
        let payload =
            serde_json::from_value(serde_json::json!({"translation_language": code})).unwrap();
        assert!(
            update_settings(State(pool.clone()), AuthUser(user_id), AppJson(payload))
                .await
                .is_err()
        );
    }
    let payload = serde_json::from_value(serde_json::json!({"red_threshold": 75})).unwrap();
    let _ = update_settings(State(pool.clone()), AuthUser(user_id), AppJson(payload))
        .await
        .unwrap();
    assert_eq!(
        get_settings(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .core
            .translation_language
            .as_deref(),
        Some("kor")
    );
    let error =
        sqlx::query("UPDATE users_settings SET translation_language = 'zzz' WHERE user_id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
}

#[tokio::test]
async fn thresholds_stay_in_range_and_ordered_against_saved_values() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let patch = |body: serde_json::Value| {
        let pool = pool.clone();
        async move {
            update_settings(
                State(pool),
                AuthUser(user_id),
                AppJson(serde_json::from_value(body).unwrap()),
            )
            .await
        }
    };
    // Start from a known saved pair.
    let saved = patch(serde_json::json!({"red_threshold": 60, "yellow_threshold": 75}))
        .await
        .unwrap()
        .0;
    assert_eq!(
        (saved.core.red_threshold, saved.core.yellow_threshold),
        (60, 75)
    );
    for body in [
        serde_json::json!({"red_threshold": -1}),
        serde_json::json!({"yellow_threshold": 101}),
        // Crossing the saved yellow (75) or saved red (60) with a partial update.
        serde_json::json!({"red_threshold": 75}),
        serde_json::json!({"yellow_threshold": 60}),
        serde_json::json!({"red_threshold": 50, "yellow_threshold": 50}),
    ] {
        let result = patch(body.clone()).await;
        assert!(matches!(result, Err(AppError::BadRequest(_))), "{body}");
    }
    let unchanged = get_settings(State(pool.clone()), AuthUser(user_id))
        .await
        .unwrap()
        .0;
    assert_eq!(
        (
            unchanged.core.red_threshold,
            unchanged.core.yellow_threshold
        ),
        (60, 75)
    );
    // Moving both together past the old values is fine when the pair is ordered.
    let moved = patch(serde_json::json!({"red_threshold": 80, "yellow_threshold": 95}))
        .await
        .unwrap()
        .0;
    assert_eq!(
        (moved.core.red_threshold, moved.core.yellow_threshold),
        (80, 95)
    );
    let partial = patch(serde_json::json!({"red_threshold": 0}))
        .await
        .unwrap()
        .0;
    assert_eq!(
        (partial.core.red_threshold, partial.core.yellow_threshold),
        (0, 95)
    );
}
