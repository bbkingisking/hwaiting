use super::*;
use crate::{
    auth::AuthUser,
    test_support::{test_pool, test_user},
};

#[tokio::test]
async fn suppressing_missing_card_returns_404_without_a_flag() {
    let pool = test_pool().await;
    let user_id = test_user(&pool).await;
    let result = suppress_card(State(pool.clone()), AppPath(999999), AuthUser(user_id)).await;
    assert!(matches!(result, Err(AppError::NotFound)));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users_card_flags WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let response = suppress_card(State(pool.clone()), AppPath(1), AuthUser(user_id))
        .await
        .unwrap();
    assert!(response.0.success);
    let suppressed: bool = sqlx::query_scalar(
        "SELECT suppressed FROM users_card_flags WHERE user_id = ? AND card_id = 1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(suppressed);
}
