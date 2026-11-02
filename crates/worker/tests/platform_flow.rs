//! Real isolated Caddy server + Docker images: swap, rollback, stop, retention and removal.
use anyhow::{Context, Result, ensure};
use forge_deploy_core::{CreateApplication, DeploymentStatus as Stage};
use forge_deploy_db::{self as db, Deployment};
use forge_deploy_docker::{Engine, image_tag};
use forge_deploy_worker::{command, process, routing::Routing};
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::HashSet, time::Duration};
use tokio::process::Command;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL, Docker and local Caddy admin; builds small BusyBox fixtures"]
async fn platform_flow() -> Result<()> {
    let url = std::env::var("TEST_DATABASE_URL")?;
    let engine = Engine::connect(
        std::env::var("FORGE_DOCKER_SOCKET").unwrap_or_else(|_| "/var/run/docker.sock".into()),
    )
    .await?;
    let admin = std::env::var("FORGE_CADDY_URL").unwrap_or_else(|_| "http://127.0.0.1:2019".into());
    let port_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = port_listener.local_addr()?.port();
    drop(port_listener);
    let schema = format!("test_{}", Uuid::new_v4().simple());
    let server = schema.clone();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    client.put(format!("{admin}/config/apps/http/servers/{server}"))
        .json(&json!({"listen":[format!("127.0.0.1:{port}")],"automatic_https":{"disable":true},"routes":[{"handle":[{"handler":"static_response","status_code":404}]}]}))
        .send().await?.error_for_status()?;
    let routing = Routing::new(admin.clone(), port)?.with_server(server.clone())?;
    let db_admin = PgPool::connect(&url).await?;
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&db_admin)
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
    let result = exercise(&pool, &engine, &routing, &client).await;
    let jobs = sqlx::query_as::<_, Deployment>("SELECT * FROM deployments")
        .fetch_all(&pool)
        .await?;
    for job in &jobs {
        engine.cleanup(job.id).await?;
    }
    let images: HashSet<_> = jobs.iter().filter_map(|j| j.image_tag.as_ref()).collect();
    for image in images {
        engine.remove_owned_image(image).await?;
    }
    client
        .delete(format!("{admin}/config/apps/http/servers/{server}"))
        .send()
        .await?
        .error_for_status()?;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&db_admin)
        .await?;
    result
}

async fn seed_image(
    pool: &PgPool,
    engine: &Engine,
    app: Uuid,
    version: usize,
) -> Result<Deployment> {
    db::enqueue(pool, app).await?;
    let job = db::claim(pool).await?.context("seed claim")?;
    let context = tempfile::tempdir()?;
    std::fs::write(
        context.path().join("Dockerfile"),
        "FROM busybox:1.37.0\nCOPY index.html /www/index.html\nCMD [\"httpd\",\"-f\",\"-p\",\"3000\",\"-h\",\"/www\"]\n",
    )?;
    std::fs::write(
        context.path().join("index.html"),
        format!("version-{version}"),
    )?;
    let mut build = Command::new("docker");
    build
        .args([
            "--host",
            &format!("unix://{}", engine.socket),
            "build",
            "--tag",
            &image_tag(job.id),
            "--label",
            &format!("forge-deploy.deployment={}", job.id),
            ".",
        ])
        .current_dir(context.path())
        .env_remove("DOCKER_CONTEXT");
    command::run(pool, job.id, build, "fixture build failed").await?;
    db::record_build(pool, job.id, &format!("{version:040x}"), &image_tag(job.id)).await?;
    // Seed retained artifacts, as if their historical containers had already been removed.
    for (from, to) in [
        (Stage::Fetching, Stage::Building),
        (Stage::Building, Stage::Starting),
        (Stage::Starting, Stage::CheckingHealth),
        (Stage::CheckingHealth, Stage::Ready),
    ] {
        db::advance(pool, job.id, from, to).await?;
    }
    db::deployment(pool, job.id)
        .await?
        .context("seed deployment")
}

async fn action(
    pool: &PgPool,
    engine: &Engine,
    routing: &Routing,
    app: Uuid,
    operation: &str,
    source: Option<Uuid>,
) -> Result<Deployment> {
    db::enqueue_action(pool, app, operation, source)
        .await?
        .context("action rejected")?;
    let job = db::claim(pool).await?.context("action claim")?;
    process(pool, engine, &job, Duration::from_secs(90), Some(routing)).await?;
    let job = db::deployment(pool, job.id)
        .await?
        .context("action result")?;
    ensure!(
        job.status == "ready",
        "{} failed: {:?}",
        operation,
        job.error
    );
    Ok(job)
}

async fn fetch(client: &reqwest::Client, port: u16) -> Result<(u16, String)> {
    let response = client
        .get(format!("http://127.0.0.1:{port}/"))
        .header("Host", "platform-test.localhost")
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

async fn exercise(
    pool: &PgPool,
    engine: &Engine,
    routing: &Routing,
    client: &reqwest::Client,
) -> Result<()> {
    db::migrate(pool).await?;
    let app = db::create_application(
        pool,
        &CreateApplication {
            name: "platform-test".into(),
            repository_url: "https://github.com/example/app".into(),
            container_port: 3000,
        },
    )
    .await?;
    let mut sources = Vec::new();
    for version in 0..5 {
        sources.push(seed_image(pool, engine, app.id, version).await?);
    }
    routing.sync(pool, None).await?;
    let first = action(
        pool,
        engine,
        routing,
        app.id,
        "rollback",
        Some(sources[1].id),
    )
    .await?;
    ensure!(fetch(client, routing.port).await? == (200, "version-1".into()));
    let poll_client = client.clone();
    let port = routing.port;
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let mut reconcile_stop = stop_tx.subscribe();
    let reconcile_pool = pool.clone();
    let reconcile_routing = routing.clone();
    let reconciliation = tokio::spawn(async move {
        loop {
            tokio::select! {
                _=reconcile_stop.changed()=>break,
                result=reconcile_routing.sync(&reconcile_pool,None)=>{result?;},
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, anyhow::Error>(())
    });
    let traffic = tokio::spawn(async move {
        let mut count = 0;
        loop {
            tokio::select! {
                _=stop_rx.changed()=>break,
                result=fetch(&poll_client,port)=>{let (status,body)=result?;ensure!(status==200 && ["version-1","version-2"].contains(&body.as_str()),"traffic interrupted");count+=1;},
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, anyhow::Error>(count)
    });
    let second_result = action(
        pool,
        engine,
        routing,
        app.id,
        "rollback",
        Some(sources[2].id),
    )
    .await;
    let _ = stop_tx.send(true);
    let count = traffic.await??;
    reconciliation.await??;
    let second = second_result?;
    ensure!(count > 0, "no requests observed during swap");
    ensure!(fetch(client, routing.port).await? == (200, "version-2".into()));
    ensure!(
        second.image_tag == sources[2].image_tag,
        "rollback rebuilt instead of reusing image"
    );
    ensure!(
        db::application(pool, app.id)
            .await?
            .context("app")?
            .active_deployment_id
            == Some(second.id)
    );
    ensure!(
        engine.running(first.id).await.is_err(),
        "old container not retired"
    );
    // A reset/restarted proxy is reconstructed from canonical database state.
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(pool)
        .await?;
    let admin = std::env::var("FORGE_CADDY_URL").unwrap_or_else(|_| "http://127.0.0.1:2019".into());
    client.patch(format!("{admin}/config/apps/http/servers/{schema}")).json(&json!({"listen":[format!("127.0.0.1:{}",routing.port)],"automatic_https":{"disable":true},"routes":[{"handle":[{"handler":"static_response","status_code":404}]}]})).send().await?.error_for_status()?;
    ensure!(fetch(client, routing.port).await?.0 == 404);
    routing.sync(pool, None).await?;
    ensure!(fetch(client, routing.port).await? == (200, "version-2".into()));
    // Lost proxy acknowledgement must preserve the candidate until reconciliation.
    let unavailable = std::net::TcpListener::bind("127.0.0.1:0")?;
    let bad_port = unavailable.local_addr()?.port();
    drop(unavailable);
    let bad_routing = Routing::new(format!("http://127.0.0.1:{bad_port}"), routing.port)?;
    db::enqueue_action(pool, app.id, "rollback", Some(sources[1].id)).await?;
    let uncertain = db::claim(pool).await?.context("uncertain claim")?;
    ensure!(
        process(
            pool,
            engine,
            &uncertain,
            Duration::from_secs(90),
            Some(&bad_routing)
        )
        .await
        .is_err()
    );
    ensure!(
        db::deployment(pool, uncertain.id)
            .await?
            .context("uncertain result")?
            .status
            == "checking_health"
    );
    ensure!(
        engine.running(uncertain.id).await?,
        "uncertain candidate was removed"
    );
    ensure!(fetch(client, routing.port).await? == (200, "version-2".into()));
    routing.sync(pool, None).await?;
    forge_deploy_worker::recover(pool, engine).await?;
    ensure!(
        db::deployment(pool, uncertain.id)
            .await?
            .context("recovered result")?
            .status
            == "failed"
    );
    ensure!(engine.running(uncertain.id).await.is_err());
    engine
        .remove_owned_image(sources[3].image_tag.as_deref().context("source image")?)
        .await?;
    db::enqueue_action(pool, app.id, "rollback", Some(sources[3].id)).await?;
    let missing = db::claim(pool).await?.context("missing-image claim")?;
    process(
        pool,
        engine,
        &missing,
        Duration::from_secs(90),
        Some(routing),
    )
    .await?;
    ensure!(
        db::deployment(pool, missing.id)
            .await?
            .context("missing")?
            .status
            == "failed"
    );
    ensure!(
        fetch(client, routing.port).await? == (200, "version-2".into()),
        "failed publication changed active route"
    );
    action(pool, engine, routing, app.id, "stop", None).await?;
    ensure!(fetch(client, routing.port).await?.0 == 404);
    ensure!(
        db::application(pool, app.id)
            .await?
            .context("stopped app")?
            .active_deployment_id
            .is_none()
    );
    action(
        pool,
        engine,
        routing,
        app.id,
        "rollback",
        Some(sources[1].id),
    )
    .await?;
    ensure!(fetch(client, routing.port).await? == (200, "version-1".into()));
    action(pool, engine, routing, app.id, "cleanup", None).await?;
    ensure!(
        !db::deployment(pool, sources[0].id)
            .await?
            .context("old image")?
            .image_available
    );
    ensure!(
        !engine
            .image_exists(sources[0].image_tag.as_deref().context("old image tag")?)
            .await?
    );
    ensure!(
        fetch(client, routing.port).await? == (200, "version-1".into()),
        "cleanup changed active version"
    );
    action(pool, engine, routing, app.id, "delete", None).await?;
    ensure!(db::application(pool, app.id).await?.is_none());
    ensure!(fetch(client, routing.port).await?.0 == 404);
    for source in sources {
        ensure!(
            !engine
                .image_exists(source.image_tag.as_deref().context("source tag")?)
                .await?
        );
    }
    Ok(())
}
