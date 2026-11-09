use super::*;

fn row(card_id: i64, rating: &str, reviewed_at: &str) -> (i64, String, String) {
    (card_id, rating.to_string(), reviewed_at.to_string())
}

#[test]
fn single_card_multiple_reviews_computes_delta_t() {
    let rows = vec![
        row(1, "good", "2026-01-01 00:00:00"),
        row(1, "good", "2026-01-04 00:00:00"),
    ];
    let items = build_fsrs_items(&rows).unwrap();
    assert_eq!(
        items,
        vec![FSRSItem {
            reviews: vec![
                FSRSReview {
                    rating: 3,
                    delta_t: 0
                },
                FSRSReview {
                    rating: 3,
                    delta_t: 3
                },
            ],
        }]
    );
}

#[test]
fn groups_across_card_boundaries() {
    let rows = vec![
        row(1, "good", "2026-01-01 00:00:00"),
        row(1, "good", "2026-01-03 00:00:00"),
        row(2, "again", "2026-01-01 00:00:00"),
        row(2, "good", "2026-01-05 00:00:00"),
    ];
    let items = build_fsrs_items(&rows).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].reviews.len(), 2);
    assert_eq!(items[1].reviews.len(), 2);
    assert_eq!(items[1].reviews[0].rating, 1);
    assert_eq!(items[1].reviews[1].delta_t, 4);
}

#[test]
fn unknown_rating_is_skipped_without_breaking_the_group() {
    let rows = vec![
        row(1, "good", "2026-01-01 00:00:00"),
        row(1, "bogus", "2026-01-02 00:00:00"),
        row(1, "good", "2026-01-04 00:00:00"),
    ];
    let items = build_fsrs_items(&rows).unwrap();
    assert_eq!(items.len(), 1);
    // The skipped row never touches last_review_time, so delta_t is
    // measured from the last *valid* review (1/1), not the skipped one.
    assert_eq!(
        items[0].reviews,
        vec![
            FSRSReview {
                rating: 3,
                delta_t: 0
            },
            FSRSReview {
                rating: 3,
                delta_t: 3
            },
        ]
    );
}

#[test]
fn same_day_only_reviews_are_filtered_out() {
    // Every review of card 1 has delta_t == 0 (all on the same day) -
    // FSRS can't train on that, so the item is dropped entirely.
    let rows = vec![
        row(1, "good", "2026-01-01 08:00:00"),
        row(1, "good", "2026-01-01 20:00:00"),
    ];
    let items = build_fsrs_items(&rows).unwrap();
    assert!(items.is_empty());
}

#[test]
fn first_review_of_a_card_always_has_delta_t_zero() {
    let rows = vec![row(1, "easy", "2026-01-01 00:00:00")];
    let items = build_fsrs_items(&rows).unwrap();
    // A lone review has delta_t 0 and so is filtered by the
    // no-positive-delta_t rule - documents that a single review never
    // contributes a training item on its own.
    assert!(items.is_empty());
}

#[test]
fn unparseable_timestamp_is_an_error() {
    let rows = vec![row(1, "good", "not a date")];
    assert!(build_fsrs_items(&rows).is_err());
}

#[test]
fn every_delayed_review_is_a_target_and_same_day_reviews_remain_history() {
    let items = build_fsrs_items(&[
        row(1, "good", "2026-01-01 08:00:00"),
        row(1, "again", "2026-01-03 08:00:00"),
        row(1, "good", "2026-01-03 09:00:00"),
        row(1, "good", "2026-01-04 10:00:00"),
        row(1, "again", "2026-01-04 11:00:00"),
    ])
    .unwrap();
    assert_eq!(
        items,
        vec![
            FSRSItem {
                reviews: vec![
                    FSRSReview {
                        rating: 3,
                        delta_t: 0
                    },
                    FSRSReview {
                        rating: 1,
                        delta_t: 2
                    },
                ]
            },
            FSRSItem {
                reviews: vec![
                    FSRSReview {
                        rating: 3,
                        delta_t: 0
                    },
                    FSRSReview {
                        rating: 1,
                        delta_t: 2
                    },
                    FSRSReview {
                        rating: 3,
                        delta_t: 0
                    },
                    FSRSReview {
                        rating: 3,
                        delta_t: 1
                    },
                ]
            },
        ]
    );
}

#[test]
fn examples_are_ordered_by_review_time_not_card_for_recency_weighting() {
    let items = build_fsrs_items(&[
        row(1, "good", "2026-01-01 00:00:00"),
        row(1, "again", "2026-01-10 00:00:00"),
        row(2, "good", "2026-01-02 00:00:00"),
        row(2, "easy", "2026-01-03 00:00:00"),
        row(3, "good", "2026-01-04 00:00:00"),
        row(3, "hard", "2026-01-05 00:00:00"),
    ])
    .unwrap();
    assert_eq!(
        items
            .iter()
            .map(|i| i.reviews.last().unwrap().rating)
            .collect::<Vec<_>>(),
        vec![4, 2, 1]
    );
}

#[test]
fn mixed_timestamp_formats_are_sorted_as_instants_before_computing_elapsed_days() {
    let items = build_fsrs_items(&[
        row(1, "again", "2026-01-03T00:00:00+02:00"), // Jan 2 at 22:00 UTC
        row(1, "easy", "2026-01-02 23:00:00"),
        row(1, "good", "2026-01-01T20:00:00Z"),
        row(1, "hard", "2026-01-04 23:00:00"),
    ])
    .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[1].reviews,
        vec![
            FSRSReview {
                rating: 3,
                delta_t: 0
            },
            FSRSReview {
                rating: 1,
                delta_t: 1
            },
            FSRSReview {
                rating: 4,
                delta_t: 0
            },
            FSRSReview {
                rating: 2,
                delta_t: 2
            },
        ]
    );
}

#[test]
fn equal_timestamp_reviews_keep_their_database_order() {
    let items = build_fsrs_items(&[
        row(1, "good", "2026-01-01 00:00:00"),
        row(1, "again", "2026-01-01T00:00:00Z"),
        row(1, "easy", "2026-01-03 00:00:00"),
    ])
    .unwrap();
    assert_eq!(
        items[0]
            .reviews
            .iter()
            .map(|r| r.rating)
            .collect::<Vec<_>>(),
        vec![3, 1, 4]
    );
}

#[tokio::test]
async fn optimization_counts_targets_once_and_reschedules_without_adding_reviews() {
    use crate::{
        auth::AuthUser,
        test_support::{test_pool, test_user},
    };
    let pool = test_pool().await;
    let uid = test_user(&pool).await;
    sqlx::query("INSERT INTO users_settings(user_id,desired_retention) VALUES (?,0.99)")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cards_states(user_id,card_id,stability,difficulty,last_review,state,review_revision,due_at) VALUES (?,1,5,3,'2026-01-05 00:00:00','review',3,0)")
        .bind(uid).execute(&pool).await.unwrap();
    for (date, rating) in [
        ("2026-01-01 00:00:00", "good"),
        ("2026-01-03 00:00:00", "again"),
        ("2026-01-05 00:00:00", "good"),
    ] {
        sqlx::query(
            "INSERT INTO review_history(user_id,card_id,rating,reviewed_at) VALUES (?,1,?,?)",
        )
        .bind(uid)
        .bind(rating)
        .bind(date)
        .execute(&pool)
        .await
        .unwrap();
    }
    // A small sample uses the crate's default-parameter fallback. This
    // exercises the real fitting task and its commit/reschedule path.
    let response = optimize_fsrs(State(pool.clone()), AuthUser(uid))
        .await
        .unwrap()
        .0;
    assert_eq!(response.review_count, 2); // Not 1 card, 3 rows, or 2+3 prefix rows.
    let parameters: String =
        sqlx::query_scalar("SELECT parameters FROM users_fsrs_parameters WHERE user_id=?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<f32>>(&parameters).unwrap(),
        response.parameters
    );
    let scheduler = super::super::scheduling::Scheduler::new(Some(&parameters), 0.99).unwrap();
    let (deadline, stability, difficulty, revision): (i64, f64, f64, i64) = sqlx::query_as(
        "SELECT due_at,stability,difficulty,review_revision FROM cards_states WHERE user_id=?",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        deadline,
        scheduler
            .due_at(parse_flexible_datetime("2026-01-05 00:00:00").unwrap(), 5.0)
            .unwrap()
    );
    assert_eq!((stability, difficulty, revision), (5.0, 3.0, 3));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM review_history WHERE user_id=?")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
}
