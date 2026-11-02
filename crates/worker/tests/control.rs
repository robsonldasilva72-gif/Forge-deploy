//! Requires an explicit disposable PostgreSQL database, not Docker.
use anyhow::Result;
use forge_deploy_core::CreateApplication;
use forge_deploy_db as db;
use forge_deploy_worker::{acquire_lock, command};
use sqlx::{Connection, PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL; verifies worker ownership and process cancellation"]
async fn worker_control() -> Result<()> {
    let url = std::env::var("TEST_DATABASE_URL")?;
    let lock = acquire_lock(&url).await?;
    assert!(acquire_lock(&url).await.is_err());
    lock.close().await?;
    acquire_lock(&url).await?.close().await?;
    let admin = PgPool::connect(&url).await?;
    let schema = format!("test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await?;
    let statement = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .after_connect(move |connection, _| {
            let statement = statement.clone();
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

async fn exercise(pool: &PgPool) -> Result<()> {
    db::migrate(pool).await?;
    let app = db::create_application(
        pool,
        &CreateApplication {
            name: "worker-test".into(),
            repository_url: "https://github.com/example/api".into(),
            container_port: 3000,
        },
    )
    .await?;
    let job = db::enqueue(pool, app.id).await?.unwrap();
    let mut command = Command::new("sh");
    command.args(["-c", "printf stdout; printf stderr >&2; exit 7"]);
    assert!(
        command::run(pool, job.id, command, "expected exit failure")
            .await
            .is_err()
    );
    let logs = db::logs(pool, job.id).await?.unwrap().logs;
    assert!(logs.contains("stdout") && logs.contains("stderr"));
    let mut command = Command::new("sh");
    // Emit the process group ID, then wait with a child process holding the pipes open.
    command.args(["-c", "printf '\nPID=%s\n' $$; sleep 60"]);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(400),
            command::run(pool, job.id, command, "timeout")
        )
        .await
        .is_err()
    );
    let logs = db::logs(pool, job.id).await?.unwrap().logs;
    let pid = logs
        .lines()
        .find_map(|line| line.strip_prefix("PID="))
        .unwrap()
        .parse::<i32>()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let alive = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok();
        if !alive {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out command survived cancellation"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}
