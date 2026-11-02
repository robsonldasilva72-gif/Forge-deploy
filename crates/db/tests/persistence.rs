//! Explicit PostgreSQL integration test; never falls back to a development database.
use forge_deploy_core::CreateApplication;
use forge_deploy_db as db;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn persistence() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("TEST_DATABASE_URL")?;
    let admin = PgPool::connect(&url).await?;
    let schema = format!("test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await?;
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .after_connect(move |connection, _| {
            let statement = search_path.clone();
            Box::pin(async move {
                sqlx::query(&statement).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await?;
    let result = exercise(&pool).await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await?;
    result
}

async fn exercise(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    db::migrate(pool).await?;
    db::migrate(pool).await?;
    let input = CreateApplication {
        name: "example-api".into(),
        repository_url: "https://github.com/example/api.git".into(),
        container_port: 3000,
    };
    input.validate()?;
    let app = db::create_application(pool, &input).await?;
    assert_eq!(db::list_applications(pool).await?.len(), 1);
    assert!(db::application_exists(pool, app.id).await?);
    let duplicate = db::create_application(pool, &input).await.unwrap_err();
    assert!(duplicate.as_database_error().unwrap().is_unique_violation());
    assert!(db::enqueue(pool, Uuid::new_v4()).await?.is_none());
    let deployment = db::enqueue(pool, app.id).await?.unwrap();
    assert_eq!(deployment.status, "queued");
    assert_eq!(
        db::deployment(pool, deployment.id)
            .await?
            .unwrap()
            .application_id,
        app.id
    );
    assert_eq!(db::list_deployments(pool, app.id).await?.len(), 1);
    assert!(db::deployment(pool, Uuid::new_v4()).await?.is_none());
    assert!(
        sqlx::query("UPDATE deployments SET status = 'invalid'")
            .execute(pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE applications SET container_port = 0")
            .execute(pool)
            .await
            .is_err()
    );
    use forge_deploy_core::DeploymentStatus as Stage;
    assert_eq!(deployment.repository_url, input.repository_url);
    // Queue entries retain the input snapshot even if application config changes later.
    sqlx::query(
        "UPDATE applications SET repository_url = 'https://github.com/example/new' WHERE id = $1",
    )
    .bind(app.id)
    .execute(pool)
    .await?;
    let second = db::enqueue(pool, app.id).await?.unwrap();
    assert_ne!(second.repository_url, deployment.repository_url);
    let (first_claim, second_claim) = tokio::join!(db::claim(pool), db::claim(pool));
    let first_claim = first_claim?.unwrap();
    let second_claim = second_claim?.unwrap();
    assert_ne!(first_claim.id, second_claim.id);
    assert!(db::claim(pool).await?.is_none());
    assert!(
        db::advance(pool, first_claim.id, Stage::Fetching, Stage::Ready)
            .await
            .is_err()
    );
    db::advance(pool, first_claim.id, Stage::Fetching, Stage::Building).await?;
    assert!(
        db::advance(pool, first_claim.id, Stage::Fetching, Stage::Building)
            .await
            .is_err()
    );
    db::record_build(pool, first_claim.id, &"a".repeat(40), "forge-deploy:test").await?;
    db::advance(pool, first_claim.id, Stage::Building, Stage::Starting).await?;
    db::record_container(pool, first_claim.id, "container-test", 12345).await?;
    db::advance(pool, first_claim.id, Stage::Starting, Stage::CheckingHealth).await?;
    db::advance(pool, first_claim.id, Stage::CheckingHealth, Stage::Ready).await?;
    db::fail(pool, first_claim.id, "must not overwrite success").await?;
    let ready = db::deployment(pool, first_claim.id).await?.unwrap();
    assert_eq!(ready.status, "ready");
    assert_eq!(ready.host_port, Some(12345));
    assert!(ready.started_at.is_some() && ready.finished_at.is_some());
    db::append_log(pool, second_claim.id, &"🦀".repeat(20000)).await?;
    db::append_log(pool, second_claim.id, "\0tail").await?;
    let logs = db::logs(pool, second_claim.id).await?.unwrap().logs;
    assert!(logs.len() <= 65536);
    assert!(logs.starts_with("[older output truncated]"));
    assert!(logs.ends_with("tail"));
    assert_eq!(db::interrupted(pool).await?.len(), 1);
    db::fail(pool, second_claim.id, "interrupted").await?;
    assert!(db::interrupted(pool).await?.is_empty());
    let failed = db::deployment(pool, second_claim.id).await?.unwrap();
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.error.as_deref(), Some("interrupted"));
    assert!(failed.host_port.is_none());
    db::runtime_logs(pool, second_claim.id, &"🦀".repeat(10000)).await?;
    assert!(
        db::logs(pool, second_claim.id)
            .await?
            .unwrap()
            .runtime_logs
            .len()
            <= 32000
    );
    let other = db::create_application(
        pool,
        &CreateApplication {
            name: "other-app".into(),
            repository_url: input.repository_url.clone(),
            container_port: 3000,
        },
    )
    .await?;
    assert!(
        db::enqueue_action(pool, other.id, "rollback", Some(first_claim.id))
            .await?
            .is_none()
    );
    Ok(())
}
