use super::*;
use crate::test_support::{test_pool, test_user};

#[tokio::test]
async fn today_has_exact_activity_contract_and_null_accuracy_without_reviews() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    let response = get_today_stats(State(pool), crate::auth::AuthUser(user))
        .await
        .unwrap()
        .0;
    let end = chrono::DateTime::parse_from_rfc3339(&response.day_ends_at).unwrap();
    assert!(end > Utc::now());
    assert_eq!(
        response.study_day,
        logical_today_date(Utc::now().naive_utc(), 4).to_string()
    );
    let mut json = serde_json::to_value(response).unwrap();
    let fields = json.as_object_mut().unwrap();
    fields.remove("study_day");
    fields.remove("day_ends_at");
    assert_eq!(
        json,
        serde_json::json!({
            "reviews_today": 0, "correct_today": 0, "percentage": null, "new_today_count": 0
        })
    );
}

#[tokio::test]
async fn today_counts_views_separately_and_normalizes_exclusive_day_bounds() {
    let pool = test_pool().await;
    let user = test_user(&pool).await;
    let other = test_user(&pool).await;
    let zone = chrono_tz::Asia::Seoul;
    sqlx::query("INSERT INTO users_settings(user_id,study_timezone,daily_new_card_limit) VALUES (?,'Asia/Seoul',0)")
        .bind(user).execute(&pool).await.unwrap();
    let now = Utc::now().with_timezone(&zone);
    let start = logical_day_start(&now, 4, 0);
    let end = logical_day_start(&now, 4, 1);
    for (owner, card, state, rating, time) in [
        (
            user,
            1,
            "learning",
            "good",
            (start - chrono::Duration::seconds(1)).to_rfc3339(),
        ),
        (user, 1, "review", "good", sqlite_datetime(start)),
        (user, 2, "learning", "good", start.to_rfc3339()),
        (
            user,
            2,
            "review",
            "again",
            sqlite_datetime(start + chrono::Duration::seconds(1)),
        ),
        (user, 3, "learning", "good", end.to_rfc3339()),
        (other, 3, "review", "good", sqlite_datetime(start)),
    ] {
        sqlx::query("INSERT INTO review_history(user_id,card_id,state,rating,reviewed_at) VALUES (?,?,?,?,?)")
            .bind(owner).bind(card).bind(state).bind(rating).bind(time).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO users_card_flags(user_id,card_id,suppressed) VALUES (?,2,1)")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let response = get_today_stats(State(pool), crate::auth::AuthUser(user))
        .await
        .unwrap()
        .0;
    assert_eq!(
        response.study_day,
        logical_today_date(now.naive_local(), 4).to_string()
    );
    assert_eq!(
        response.day_ends_at,
        end.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    );
    assert_eq!(response.reviews_today, 2);
    assert_eq!(response.correct_today, 1);
    assert_eq!(response.percentage, Some(50.0));
    assert_eq!(response.new_today_count, 1);
}
