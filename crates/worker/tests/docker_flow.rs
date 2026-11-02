//! Real Git -> Docker -> HTTP validation. Explicit opt-in; pulls/builds public Docker sample.
use anyhow::{Context, Result, ensure};
use forge_deploy_core::CreateApplication;
use forge_deploy_db as db;
use forge_deploy_docker::Engine;
use forge_deploy_worker::{process, recover};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL, Docker access and internet; builds docker/welcome-to-docker"]
async fn real_deployment_flow() -> Result<()> {
    let url = std::env::var("TEST_DATABASE_URL")?;
    let engine = Engine::connect(
        std::env::var("FORGE_DOCKER_SOCKET").unwrap_or_else(|_| "/var/run/docker.sock".into()),
    )
    .await?;
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
    let result = exercise(&pool, &engine).await;
    // Remove only containers belonging to this test schema. Keep image cache; never prune globally.
    let ids = sqlx::query_scalar::<_, Uuid>("SELECT id FROM deployments")
        .fetch_all(&pool)
        .await?;
    let mut cleanup_error = None;
    for id in ids {
        if let Err(error) = engine.cleanup(id).await {
            cleanup_error = Some(error);
        }
    }
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await?;
    result?;
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    Ok(())
}

async fn exercise(pool: &PgPool, engine: &Engine) -> Result<()> {
    db::migrate(pool).await?;
    let app = db::create_application(
        pool,
        &CreateApplication {
            name: "docker-sample".into(),
            repository_url: "https://github.com/docker/welcome-to-docker.git".into(),
            container_port: 3000,
        },
    )
    .await?;
    let queued = db::enqueue(pool, app.id).await?.context("enqueue")?;
    let job = db::claim(pool).await?.context("claim")?;
    ensure!(job.id == queued.id);
    process(pool, engine, &job, Duration::from_secs(600), None).await?;
    let ready = db::deployment(pool, job.id).await?.context("deployment")?;
    ensure!(
        ready.status == "ready",
        "Expected ready, got {:?}; logs: {}",
        ready.error,
        db::logs(pool, job.id).await?.context("logs")?.logs
    );
    ensure!(ready.commit_sha.as_ref().is_some_and(|s| s.len() == 40));
    ensure!(ready.container_id.is_some() && ready.image_tag.is_some());
    let port = ready.host_port.context("host port")?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await?;
    ensure!(response.status().is_success());
    ensure!(engine.running(job.id).await?);

    // A missing Dockerfile fails, and cannot affect the previously published application.
    let invalid = db::create_application(
        pool,
        &CreateApplication {
            name: "no-dockerfile".into(),
            repository_url: "https://github.com/octocat/Hello-World.git".into(),
            container_port: 3000,
        },
    )
    .await?;
    db::enqueue(pool, invalid.id).await?;
    let failed_job = db::claim(pool).await?.context("claim invalid")?;
    process(pool, engine, &failed_job, Duration::from_secs(120), None).await?;
    let failed = db::deployment(pool, failed_job.id)
        .await?
        .context("failed deployment")?;
    ensure!(failed.status == "failed");
    ensure!(failed.error.as_deref() == Some("Dockerfile não encontrado na raiz do repositório"));
    ensure!(failed.host_port.is_none());

    // A container that listens on a different port must fail HTTP verification and be removed.
    sqlx::query("UPDATE applications SET container_port = 65534 WHERE id = $1")
        .bind(app.id)
        .execute(pool)
        .await?;
    db::enqueue(pool, app.id).await?;
    let unhealthy = db::claim(pool).await?.context("claim unhealthy")?;
    process(pool, engine, &unhealthy, Duration::from_secs(600), None).await?;
    let failed = db::deployment(pool, unhealthy.id)
        .await?
        .context("unhealthy deployment")?;
    ensure!(failed.status == "failed" && failed.host_port.is_none());
    ensure!(
        failed.error.as_deref()
            == Some("Aplicação não respondeu HTTP 2xx em / dentro de 30 segundos")
    );
    ensure!(
        engine.running(unhealthy.id).await.is_err(),
        "failed container should be removed"
    );

    // Simulate a previously interrupted attempt; recovery preserves ready containers.
    db::enqueue(pool, app.id).await?;
    let interrupted = db::claim(pool).await?.context("claim interrupted")?;
    recover(pool, engine).await?;
    ensure!(
        db::deployment(pool, interrupted.id)
            .await?
            .context("interrupted")?
            .status
            == "failed"
    );
    ensure!(
        engine.running(job.id).await?,
        "recovery removed a ready deployment"
    );

    db::enqueue(pool, app.id).await?;
    let timeout = db::claim(pool).await?.context("claim timeout")?;
    process(pool, engine, &timeout, Duration::from_nanos(1), None).await?;
    let timed_out = db::deployment(pool, timeout.id).await?.context("timeout")?;
    ensure!(timed_out.status == "failed");
    ensure!(timed_out.error.as_deref() == Some("Tempo máximo do deploy excedido"));
    Ok(())
}
