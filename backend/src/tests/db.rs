use super::*;
use crate::test_support::test_pool;

#[test]
fn migration_timezone_defaults_to_utc_without_host_files() {
    assert_eq!(migration_timezone(None).unwrap(), chrono_tz::UTC);
    for name in ["Europe/London", ":Europe/London"] {
        assert_eq!(
            migration_timezone(Some(name)).unwrap(),
            chrono_tz::Europe::London
        );
    }
    for name in ["", "Mars/Olympus", "/etc/localtime"] {
        let error = migration_timezone(Some(name)).unwrap_err().to_string();
        assert!(error.contains("TZ="));
        assert!(error.contains("not an IANA timezone"));
    }
}

#[tokio::test]
async fn timezone_upgrade_preserves_existing_accounts_once() {
    let path = std::env::temp_dir().join(format!("hwaiting_timezone_{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite://{}", path.display());
    let old_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str(&url)
                .unwrap()
                .create_if_missing(true)
                .foreign_keys(false),
        )
        .await
        .unwrap();
    let old_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            sqlx::migrate!("./migrations")
                .iter()
                .filter(|migration| migration.version < 20261109000001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    old_migrations.run(&old_pool).await.unwrap();
    let existing = crate::test_support::test_user(&old_pool).await;
    let without_settings = crate::test_support::test_user(&old_pool).await;
    sqlx::query("INSERT INTO users_settings (user_id, day_boundary_hour) VALUES (?, 2)")
        .bind(existing)
        .execute(&old_pool)
        .await
        .unwrap();
    old_pool.close().await;

    let pool = connect_migrated(&url).await.unwrap();
    let names: Vec<String> =
        sqlx::query_scalar("SELECT study_timezone FROM users_settings ORDER BY user_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0], names[1]);
    assert!(names[0].parse::<chrono_tz::Tz>().is_ok());
    let hours: Vec<i64> =
        sqlx::query_scalar("SELECT day_boundary_hour FROM users_settings ORDER BY user_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(hours, vec![2, 4]);
    // A saved choice must survive future startups; new users remain uninitialized.
    sqlx::query("UPDATE users_settings SET study_timezone = 'Asia/Seoul' WHERE user_id = ?")
        .bind(without_settings)
        .execute(&pool)
        .await
        .unwrap();
    let new_user = crate::test_support::test_user(&pool).await;
    crate::user::ensure_settings_row(&pool, new_user)
        .await
        .unwrap();
    pool.close().await;
    let pool = connect_migrated(&url).await.unwrap();
    let names: Vec<Option<String>> =
        sqlx::query_scalar("SELECT study_timezone FROM users_settings ORDER BY user_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(names[1].as_deref(), Some("Asia/Seoul"));
    assert_eq!(names[2], None);
}

#[tokio::test]
async fn every_runtime_connection_enforces_foreign_keys() {
    let pool = test_pool().await;
    let mut connections = Vec::new();
    for _ in 0..5 {
        connections.push(pool.acquire().await.unwrap());
    }
    for connection in &mut connections {
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(enabled, 1);
    }
    drop(connections);

    let result = sqlx::query(
        "INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (999999, 999999, 1)",
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "an orphaned flag must be rejected by SQLite"
    );
}

#[tokio::test]
async fn existing_foreign_key_violations_block_startup() {
    let path = std::env::temp_dir().join(format!(
        "hwaiting_fk_test_{}_{}.db",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let database_url = format!("sqlite://{}", path.display());
    connect_migrated(&database_url).await.unwrap().close().await;

    // Simulate a row written by an older application connection with
    // foreign keys disabled, then verify the next startup catches it.
    let old_options = SqliteConnectOptions::from_str(&database_url)
        .unwrap()
        .foreign_keys(false);
    let old_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(old_options)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO users_card_flags (user_id, card_id, suppressed) VALUES (999999, 999999, 1)",
    )
    .execute(&old_pool)
    .await
    .unwrap();
    old_pool.close().await;

    let error = match connect_migrated(&database_url).await {
        Ok(_) => panic!("startup should reject the orphaned flag"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("users_card_flags"), "{error}");
    assert!(error.contains("missing"), "{error}");
}
