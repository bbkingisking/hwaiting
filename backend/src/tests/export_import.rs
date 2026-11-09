use super::*;
use crate::test_support::{test_pool, test_user};

async fn setup_source_user(pool: &SqlitePool, user_id: i64) {
    sqlx::query(
        "INSERT INTO users_settings (user_id, daily_new_card_limit, day_boundary_hour) VALUES (?, 7, 2)",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO users_fsrs_parameters (user_id, parameters) VALUES (?, '[0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2]')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    // Two reviews of card 1 (card 1 ends up "review"), one of card 2.
    sqlx::query(
        "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
         VALUES (?, 1, 'good', '2026-01-01 00:00:00', 1, 1, 'learning')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
         VALUES (?, 1, 'good', '2026-01-04 00:00:00', 3, 2, 'review')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, stability, difficulty, state) \
         VALUES (?, 2, 'again', '2026-01-02 00:00:00', 1, 1, 'learning')",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 3, 1)")
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn invalid_settings_match_patch_and_preserve_existing_data() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    setup_source_user(&pool, user_id).await;
    sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision) VALUES (?, 1, 3, 2, '2026-01-04', 'review', 7)")
        .bind(user_id).execute(&pool).await.unwrap();

    let before = serde_json::to_value(
        export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    for (field, value) in [
        ("study_timezone", serde_json::json!("Mars/Olympus")),
        ("day_boundary_hour", serde_json::json!(-1)),
        ("day_boundary_hour", serde_json::json!(24)),
        ("auto_progress_delay", serde_json::json!(-1)),
        ("auto_progress_delay", serde_json::json!(3001)),
        ("desired_retention", serde_json::json!(0.49)),
        ("desired_retention", serde_json::json!(1.0)),
        ("daily_new_card_limit", serde_json::json!(-1)),
    ] {
        let patch: crate::user::UpdateSettingsRequest =
            serde_json::from_value(serde_json::json!({field: value})).unwrap();
        let patch_error = match crate::user::update_settings(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(patch),
        )
        .await
        {
            Err(AppError::BadRequest(message)) => message,
            _ => panic!("PATCH should reject {field}={value}"),
        };
        for overwrite in [false, true] {
            let mut invalid = before.clone();
            invalid["settings"][field] = value.clone();
            // A successful overwrite would remove the existing history/flags.
            invalid["review_history"] = serde_json::json!([]);
            invalid["suppressed_cards"] = serde_json::json!([]);
            match import_data(
                State(pool.clone()),
                AuthUser(user_id),
                AppJson(ImportDataRequest {
                    data: serde_json::from_value(invalid).unwrap(),
                    overwrite,
                }),
            )
            .await
            {
                Err(AppError::BadRequest(message)) => assert_eq!(message, patch_error),
                _ => panic!("import should reject {field}={value}"),
            }
            let mut after = serde_json::to_value(
                export_data(State(pool.clone()), AuthUser(user_id))
                    .await
                    .unwrap()
                    .0,
            )
            .unwrap();
            after["exported_at"] = before["exported_at"].clone();
            assert_eq!(after, before, "invalid import must preserve existing data");
            let state: (f64, f64, String, String, i64) = sqlx::query_as(
                "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
            ).bind(user_id).fetch_one(&pool).await.unwrap();
            assert_eq!(state, (3.0, 2.0, "2026-01-04".into(), "review".into(), 7));
        }
    }
}

#[tokio::test]
async fn invalid_backup_cannot_replace_existing_history_or_state() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    setup_source_user(&pool, user_id).await;
    sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision) VALUES (?, 1, 3, 2, '2026-01-04', 'review', 7)")
        .bind(user_id).execute(&pool).await.unwrap();

    let before = serde_json::to_value(
        export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    let state_before: (f64, f64, String, String, i64) = sqlx::query_as(
        "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    for (name, path, invalid) in [
        (
            "FSRS shape",
            "fsrs_parameters",
            serde_json::json!("[1,2,3]"),
        ),
        (
            "export timestamp",
            "exported_at",
            serde_json::json!("not a date"),
        ),
        (
            "review timestamp",
            "reviewed_at",
            serde_json::json!("not a date"),
        ),
        ("rating", "rating", serde_json::json!("perfect")),
        ("scheduled days", "scheduled_days", serde_json::json!(-1)),
        ("elapsed days", "elapsed_days", serde_json::json!(-1)),
        ("zero stability", "stability", serde_json::json!(0)),
        (
            "unrepresentable stability",
            "stability",
            serde_json::json!(1e40),
        ),
        ("difficulty", "difficulty", serde_json::json!(11)),
        ("missing difficulty", "difficulty", serde_json::Value::Null),
        ("invalid state", "state", serde_json::json!("new")),
    ] {
        let mut invalid_data = before.clone();
        match path {
            "fsrs_parameters" => invalid_data["settings"][path] = invalid,
            "exported_at" => invalid_data[path] = invalid,
            _ => invalid_data["review_history"][0][path] = invalid,
        }
        let result = import_data(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(ImportDataRequest {
                data: serde_json::from_value(invalid_data).unwrap(),
                overwrite: true,
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::BadRequest(_))), "{name}");
        let mut after = serde_json::to_value(
            export_data(State(pool.clone()), AuthUser(user_id))
                .await
                .unwrap()
                .0,
        )
        .unwrap();
        after["exported_at"] = before["exported_at"].clone();
        assert_eq!(after, before, "{name} must preserve existing data");
        let state_after: (f64, f64, String, String, i64) = sqlx::query_as(
            "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state_after, state_before, "{name} must preserve card state");
    }

    // A historical row may lack memory state, but the final review of a
    // restored card must provide it. This is checked inside the transaction.
    let mut invalid_data = before.clone();
    invalid_data["review_history"][1]["stability"] = serde_json::Value::Null;
    invalid_data["review_history"][1]["difficulty"] = serde_json::Value::Null;
    invalid_data["review_history"][1]["state"] = serde_json::Value::Null;
    let result = import_data(
        State(pool.clone()),
        AuthUser(user_id),
        AppJson(ImportDataRequest {
            data: serde_json::from_value(invalid_data).unwrap(),
            overwrite: true,
        }),
    )
    .await;
    assert!(matches!(result, Err(AppError::BadRequest(_))));
    let mut after = serde_json::to_value(
        export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    after["exported_at"] = before["exported_at"].clone();
    assert_eq!(after, before);
    let state_after: (f64, f64, String, String, i64) = sqlx::query_as(
        "SELECT stability, difficulty, last_review, state, review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state_after, state_before);
}

#[tokio::test]
async fn invalid_review_sentence_identity_cannot_replace_existing_history() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    setup_source_user(&pool, user_id).await;
    let before = export_data(State(pool.clone()), AuthUser(user_id))
        .await
        .unwrap()
        .0;
    let own_sentence: i64 =
        sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = 1 LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    let other_sentence: i64 =
        sqlx::query_scalar("SELECT id FROM sentences WHERE card_id = 2 LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    for (name, sentence_id, wrong_answer) in [
        ("foreign sentence", Some(other_sentence), None),
        ("missing sentence", Some(i64::MAX), None),
        (
            "mismatched wrong answer",
            Some(own_sentence),
            Some(WrongAnswerExport {
                target_sentence_id: other_sentence,
                answer: "wrong".into(),
            }),
        ),
    ] {
        let mut data = export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0;
        data.review_history[0].sentence_id = sentence_id;
        data.review_history[0].wrong_answer = wrong_answer;
        let result = import_data(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(ImportDataRequest {
                data,
                overwrite: true,
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::BadRequest(_))), "{name}");

        let mut after = export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0;
        after.exported_at = before.exported_at.clone();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            serde_json::to_value(&before).unwrap(),
            "{name} must preserve existing history"
        );
    }
}

#[tokio::test]
async fn import_uses_latest_instant_with_mixed_timestamp_formats() {
    let pool = test_pool().await;
    let source_id = test_user(&pool).await;
    setup_source_user(&pool, source_id).await;
    let mut data = export_data(State(pool.clone()), AuthUser(source_id))
        .await
        .unwrap()
        .0;
    // The later instant sorts before the older one as text because the
    // older timestamp has a 'T' where the later one has a space.
    data.review_history[0].reviewed_at = "2026-01-03T00:00:00Z".into();
    data.review_history[0].stability = Some(1.0);
    data.review_history[2].reviewed_at = "2026-01-04 00:00:00".into();
    data.review_history[2].stability = Some(4.0);
    let dest_id = test_user(&pool).await;
    let result = import_data(
        State(pool.clone()),
        AuthUser(dest_id),
        AppJson(ImportDataRequest {
            data,
            overwrite: false,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(result.stats.card_states_derived, 2);
    let (stability, last_review): (f64, String) = sqlx::query_as(
        "SELECT stability, last_review FROM cards_states WHERE user_id = ? AND card_id = 1",
    )
    .bind(dest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stability, 4.0);
    assert_eq!(last_review, "2026-01-04 00:00:00");
}

#[tokio::test]
async fn imports_accept_settings_at_validation_boundaries() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    for (hour, delay, retention, limit) in [(0, 0, 0.5, 0), (23, 3000, 0.99, i64::MAX)] {
        let mut data = export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0;
        data.settings.core.day_boundary_hour = hour;
        data.settings.core.auto_progress_delay = delay;
        data.settings.core.desired_retention = retention;
        data.settings.core.daily_new_card_limit = limit;
        let _ = import_data(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(ImportDataRequest {
                data,
                overwrite: true,
            }),
        )
        .await
        .unwrap();
        let settings = get_user_settings(&pool, user_id).await.unwrap().core;
        assert_eq!(settings.day_boundary_hour, hour);
        assert_eq!(settings.auto_progress_delay, delay);
        assert_eq!(settings.desired_retention, retention);
        assert_eq!(settings.daily_new_card_limit, limit);
    }
}

#[tokio::test]
async fn export_then_import_round_trips_settings_history_and_suppressions() {
    let pool = test_pool().await;
    let source_id = test_user(&pool).await;
    setup_source_user(&pool, source_id).await;

    sqlx::query("UPDATE users_settings SET show_hanja_hints = 0, study_timezone = 'Asia/Seoul' WHERE user_id = ?")
        .bind(source_id)
        .execute(&pool)
        .await
        .unwrap();

    let exported = export_data(State(pool.clone()), AuthUser(source_id))
        .await
        .unwrap()
        .0;
    assert!(!exported.settings.core.show_hanja_hints);
    assert_eq!(exported.review_history.len(), 3);
    assert_eq!(exported.suppressed_cards, vec![3]);

    let dest_id = test_user(&pool).await;
    let import_result = import_data(
        State(pool.clone()),
        AuthUser(dest_id),
        AppJson(ImportDataRequest {
            data: exported,
            overwrite: false,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(import_result.stats.reviews_imported, 3);
    assert_eq!(import_result.stats.suppressed_cards_imported, 1);
    // One derived cards_states row per distinct card_id in the imported
    // history (cards 1 and 2).
    assert_eq!(import_result.stats.card_states_derived, 2);

    // Settings round-tripped, including the FSRS parameters.
    let dest_settings = export_data(State(pool.clone()), AuthUser(dest_id))
        .await
        .unwrap()
        .0
        .settings;
    assert_eq!(dest_settings.core.daily_new_card_limit, 7);
    assert_eq!(dest_settings.core.day_boundary_hour, 2);
    assert_eq!(
        dest_settings.core.study_timezone.as_deref(),
        Some("Asia/Seoul")
    );
    assert!(!dest_settings.core.show_hanja_hints);
    assert_eq!(
        dest_settings.fsrs_parameters.as_deref(),
        Some(
            "[0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2]"
        )
    );

    // cards_states derived from the *latest* review per card: card 1's
    // last review (1/4) had stability 3, not the first review's 1.
    let (stability, state): (f64, String) = sqlx::query_as(
        "SELECT stability, state FROM cards_states WHERE user_id = ? AND card_id = 1",
    )
    .bind(dest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stability, 3.0);
    assert_eq!(state, "review");

    let suppressed: Vec<i64> = sqlx::query_scalar(
        "SELECT card_id FROM users_card_flags WHERE user_id = ? AND suppressed = 1",
    )
    .bind(dest_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(suppressed, vec![3]);
}

#[tokio::test]
async fn old_backup_preserves_saved_study_timezone() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    setup_source_user(&pool, user_id).await;
    sqlx::query("UPDATE users_settings SET study_timezone = 'Europe/London' WHERE user_id = ?")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    let mut old = serde_json::to_value(
        export_data(State(pool.clone()), AuthUser(user_id))
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    old["settings"]
        .as_object_mut()
        .unwrap()
        .remove("study_timezone");
    for overwrite in [false, true] {
        let _ = import_data(
            State(pool.clone()),
            AuthUser(user_id),
            AppJson(ImportDataRequest {
                data: serde_json::from_value(old.clone()).unwrap(),
                overwrite,
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            get_user_settings(&pool, user_id)
                .await
                .unwrap()
                .core
                .study_timezone
                .as_deref(),
            Some("Europe/London")
        );
    }
}

#[tokio::test]
async fn import_fsrs_parameters_respects_overwrite_and_backup_defaults() {
    let pool = test_pool().await;
    let source_id = test_user(&pool).await;
    setup_source_user(&pool, source_id).await;

    for overwrite in [false, true] {
        for replacement in [
            None,
            Some(
                "[0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3,0.3]",
            ),
        ] {
            let dest_id = test_user(&pool).await;
            sqlx::query("INSERT INTO users_fsrs_parameters (user_id, parameters) VALUES (?, '[0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4]')")
                .bind(dest_id).execute(&pool).await.unwrap();
            let mut data = export_data(State(pool.clone()), AuthUser(source_id))
                .await
                .unwrap()
                .0;
            data.settings.fsrs_parameters = replacement.map(str::to_owned);

            let _ = import_data(
                State(pool.clone()),
                AuthUser(dest_id),
                AppJson(ImportDataRequest { data, overwrite }),
            )
            .await
            .unwrap();

            let settings = get_user_settings(&pool, dest_id).await.unwrap();
            let expected = replacement.or({
                if overwrite {
                    None
                } else {
                    Some("[0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4,0.4]")
                }
            });
            assert_eq!(settings.fsrs_parameters.as_deref(), expected);
            // Clearing the destination must leave other users' parameters intact.
            let source = get_user_settings(&pool, source_id).await.unwrap();
            assert_eq!(
                source.fsrs_parameters.as_deref(),
                Some(
                    "[0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2,0.2]"
                )
            );
        }
    }
}

#[tokio::test]
async fn overwrite_clears_existing_data_before_importing() {
    let pool = test_pool().await;
    let source_id = test_user(&pool).await;
    setup_source_user(&pool, source_id).await;
    let exported = export_data(State(pool.clone()), AuthUser(source_id))
        .await
        .unwrap()
        .0;

    let dest_id = test_user(&pool).await;
    // Pre-existing data that overwrite:true should wipe.
    sqlx::query(
        "INSERT INTO review_history (user_id, card_id, rating, reviewed_at, state) \
         VALUES (?, 10, 'easy', '2020-01-01 00:00:00', 'review')",
    )
    .bind(dest_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (?, 10, 1)")
        .bind(dest_id)
        .execute(&pool)
        .await
        .unwrap();

    let _ = import_data(
        State(pool.clone()),
        AuthUser(dest_id),
        AppJson(ImportDataRequest {
            data: exported,
            overwrite: true,
        }),
    )
    .await
    .unwrap();

    let has_old_review: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM review_history WHERE user_id = ? AND card_id = 10)",
    )
    .bind(dest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !has_old_review,
        "overwrite should have cleared the pre-existing review"
    );

    let has_old_suppression: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users_card_flags WHERE user_id = ? AND card_id = 10)",
    )
    .bind(dest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !has_old_suppression,
        "overwrite should have cleared the pre-existing suppression"
    );
}

#[tokio::test]
async fn unknown_card_ids_are_skipped_and_counted_correctly() {
    let pool = test_pool().await;
    let dest_id = test_user(&pool).await;

    let data = ExportData {
        version: "1.0".to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        settings: UserSettingsExport {
            core: crate::user::UserSettingsCore {
                show_hanja_hints: true,
                red_threshold: 50,
                yellow_threshold: 70,
                day_boundary_hour: 4,
                study_timezone: Some("Asia/Seoul".into()),
                auto_progress_on_correct: false,
                auto_progress_delay: 1500,
                desired_retention: 0.9,
                daily_new_card_limit: 20,
                history_colorized_area: false,
                history_colored_dots: false,
                history_threshold_lines: false,
            },
            fsrs_parameters: None,
        },
        review_history: vec![
            ReviewHistoryExport {
                sentence_id: None,
                card_id: 1, // real
                rating: "good".to_string(),
                scheduled_days: None,
                elapsed_days: None,
                reviewed_at: "2026-01-01 00:00:00".to_string(),
                stability: Some(1.0),
                difficulty: Some(1.0),
                state: Some("learning".to_string()),
                wrong_answer: None,
            },
            ReviewHistoryExport {
                sentence_id: None,
                card_id: 999999, // doesn't exist
                rating: "good".to_string(),
                scheduled_days: None,
                elapsed_days: None,
                reviewed_at: "2026-01-01 00:00:00".to_string(),
                stability: Some(1.0),
                difficulty: Some(1.0),
                state: Some("learning".to_string()),
                wrong_answer: None,
            },
        ],
        suppressed_cards: vec![2, 999999],
    };

    let result = import_data(
        State(pool.clone()),
        AuthUser(dest_id),
        AppJson(ImportDataRequest {
            data,
            overwrite: false,
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(result.stats.reviews_imported, 1);
    assert_eq!(result.stats.suppressed_cards_imported, 1);
    assert_eq!(result.stats.card_states_derived, 1);
    let imported_sentence: Option<i64> = sqlx::query_scalar(
        "SELECT sentence_id FROM review_history WHERE user_id = ? AND card_id = 1",
    )
    .bind(dest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(imported_sentence, Some(1));
}

#[tokio::test]
async fn imports_advance_revisions_and_overwrite_preserves_reset_markers() {
    let pool = test_pool().await;
    let source_id = test_user(&pool).await;
    setup_source_user(&pool, source_id).await;
    let dest_id = test_user(&pool).await;
    sqlx::query("INSERT INTO cards_states (user_id, card_id, stability, difficulty, last_review, state, review_revision) VALUES (?, 10, 3, 2, '2026-01-01 00:00:00', 'review', 7)")
        .bind(dest_id).execute(&pool).await.unwrap();
    // Card 1 initially has no row (revision zero). Merging must invalidate
    // that prompt; later merges and overwrites must keep advancing it.
    let mut previous_revision = 0;
    for overwrite in [false, false, true, true] {
        let data = export_data(State(pool.clone()), AuthUser(source_id))
            .await
            .unwrap()
            .0;
        let _ = import_data(
            State(pool.clone()),
            AuthUser(dest_id),
            AppJson(ImportDataRequest { data, overwrite }),
        )
        .await
        .unwrap();
        let revision: i64 = sqlx::query_scalar(
            "SELECT review_revision FROM cards_states WHERE user_id = ? AND card_id = 1",
        )
        .bind(dest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(revision > previous_revision);
        previous_revision = revision;
    }
    // Card 10 is absent from the import: its learning history is cleared,
    // but its revision must survive so a prompt with revision 7 stays stale.
    let (revision, last_review, stability, state): (i64, Option<String>, f64, String) = sqlx::query_as(
        "SELECT review_revision, last_review, stability, state FROM cards_states WHERE user_id = ? AND card_id = 10")
        .bind(dest_id).fetch_one(&pool).await.unwrap();
    assert_eq!(revision, 9);
    assert_eq!(last_review, None);
    assert_eq!(stability, 0.0);
    assert_eq!(state, "new");
    let stale = crate::cards::check_answer(
        State(pool.clone()),
        crate::error::AppPath(10),
        AuthUser(dest_id),
        AppJson(crate::cards::CheckRequest {
            answer: "test".into(),
            sentence_id: 10,
            review_revision: 7,
            content_revision: 0,
        }),
    )
    .await;
    assert!(matches!(stale, Err(AppError::StaleReview)));
}
