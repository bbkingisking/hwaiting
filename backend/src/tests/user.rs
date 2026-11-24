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
