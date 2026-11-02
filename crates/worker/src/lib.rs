//! Sequential deployment execution with session-lock ownership and persisted progress.
pub mod command;
pub mod management;
pub mod routing;

use anyhow::{Context, Result, bail};
use forge_deploy_core::{CreateApplication, DeploymentStatus as Stage};
use forge_deploy_db::{self as db, Deployment};
use forge_deploy_docker::{Engine, image_tag};
use sqlx::{Connection, PgConnection, PgPool};
use std::{path::Path, time::Duration};
use tokio::process::Command;

pub const WORKER_LOCK: i64 = 0x464f524745;

/// The connection is dedicated: returning a session lock to a connection pool is unsafe.
pub async fn acquire_lock(url: &str) -> Result<PgConnection> {
    let mut connection = PgConnection::connect(url).await?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(WORKER_LOCK)
        .fetch_one(&mut connection)
        .await?;
    if !acquired {
        bail!("Já existe um executor ativo para este banco");
    }
    Ok(connection)
}

/// A disconnected owner must stop processing rather than silently lose serialization.
pub async fn monitor_lock(connection: &mut PgConnection) -> Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        tokio::time::timeout(
            Duration::from_secs(3),
            sqlx::query("INSERT INTO worker_heartbeat (id,seen_at) VALUES (1,now()) ON CONFLICT (id) DO UPDATE SET seen_at=now()").execute(&mut *connection),
        )
        .await
        .context("Conexão do lock deixou de responder")??;
    }
}

pub async fn recover(pool: &PgPool, engine: &Engine) -> Result<()> {
    for job in db::interrupted(pool).await? {
        engine.cleanup(job.id).await?;
        db::append_log(
            pool,
            job.id,
            "\n[worker] Execução interrompida; container parcial removido.\n",
        )
        .await?;
        db::fail(
            pool,
            job.id,
            "Executor foi interrompido. Solicite um novo deploy.",
        )
        .await?;
    }
    Ok(())
}

pub async fn process(
    pool: &PgPool,
    engine: &Engine,
    job: &Deployment,
    timeout: Duration,
    routing: Option<&routing::Routing>,
) -> Result<()> {
    match tokio::time::timeout(timeout, execute(pool, engine, job, routing)).await {
        Ok(Ok(())) => Ok(()),
        outcome => {
            // A timeout can race with a completed publication. Read canonical state before cleanup.
            if db::deployment(pool, job.id)
                .await?
                .is_some_and(|d| d.status == "ready")
            {
                return Ok(());
            }
            if let Some(routing) = routing {
                routing
                    .sync(pool, None)
                    .await
                    .map_err(|_| routing::RoutingUncertain)?;
            }
            if let Ok(Err(error)) = &outcome
                && error.downcast_ref::<routing::RoutingUncertain>().is_some()
            {
                return Err(routing::RoutingUncertain.into());
            }
            let reason = match outcome {
                Err(_) => "Tempo máximo do deploy excedido".to_owned(),
                Ok(Err(error)) => error.to_string(),
                Ok(Ok(())) => unreachable!(),
            };
            // Keep recent runtime output for diagnosis before removing the failed container.
            if let Ok(Ok(logs)) =
                tokio::time::timeout(Duration::from_secs(5), engine.tail_logs(job.id)).await
            {
                db::append_log(pool, job.id, &logs).await?;
            }
            engine
                .cleanup(job.id)
                .await
                .context("Falha na limpeza; será tentada na próxima inicialização")?;
            db::append_log(pool, job.id, &format!("\n[worker] Falha: {reason}\n")).await?;
            db::fail(pool, job.id, &reason).await?;
            tracing::warn!(deployment = %job.id, "Deploy falhou; consulte o histórico");
            Ok(()) // A failed application must not stop subsequent jobs.
        }
    }
}

async fn execute(
    pool: &PgPool,
    engine: &Engine,
    job: &Deployment,
    routing: Option<&routing::Routing>,
) -> Result<()> {
    let app = db::application(pool, job.application_id)
        .await?
        .context("Aplicação foi removida")?;
    if !["deploy", "rollback"].contains(&job.operation.as_str()) {
        return management::perform(pool, engine, routing, job).await;
    }
    let input = CreateApplication {
        name: "deployment".into(),
        repository_url: job.repository_url.clone(),
        container_port: u16::try_from(job.container_port)?,
    };
    input.validate().map_err(anyhow::Error::msg)?;
    let workspace = tempfile::Builder::new().prefix("forge-deploy-").tempdir()?;
    let repository = workspace.path().join("repository");
    db::append_log(
        pool,
        job.id,
        if job.operation == "rollback" {
            "[worker] Preparando restauração da imagem.\n"
        } else {
            "[worker] Baixando branch padrão do repositório.\n"
        },
    )
    .await?;
    if job.operation == "rollback" {
        let source = db::deployment(
            pool,
            job.source_deployment_id
                .context("Versão de origem ausente")?,
        )
        .await?
        .context("Versão não encontrada")?;
        if source.application_id != job.application_id
            || source.status != "ready"
            || !source.image_available
        {
            bail!("Versão não está disponível para rollback");
        }
        let image = source.image_tag.context("Imagem não encontrada")?;
        if !engine.image_exists(&image).await? {
            bail!("Imagem foi removida do Docker");
        }
        db::record_build(
            pool,
            job.id,
            &source.commit_sha.context("Commit ausente")?,
            &image,
        )
        .await?;
        db::advance(pool, job.id, Stage::Fetching, Stage::Building).await?;
        db::append_log(
            pool,
            job.id,
            "[worker] Reutilizando imagem existente; nenhum build necessário.\n",
        )
        .await?;
    } else {
        clone_repository(pool, job, &repository).await?;
        let commit = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repository)
            .kill_on_drop(true)
            .output()
            .await?;
        if !commit.status.success() {
            bail!("Não foi possível identificar o commit");
        }
        let commit = String::from_utf8(commit.stdout)?.trim().to_owned();
        if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("Identificador de commit inválido");
        }
        if !repository.join("Dockerfile").is_file() {
            bail!("Dockerfile não encontrado na raiz do repositório");
        }
        db::record_build(pool, job.id, &commit, &image_tag(job.id)).await?;
        db::advance(pool, job.id, Stage::Fetching, Stage::Building).await?;
        let mut build = Command::new("docker");
        build
            .args([
                "--host",
                &format!("unix://{}", engine.socket),
                "build",
                "--progress=plain",
                "--tag",
                &image_tag(job.id),
                "--label",
                &format!("forge-deploy.deployment={}", job.id),
                ".",
            ])
            .current_dir(&repository)
            .env("DOCKER_BUILDKIT", "1")
            .env_remove("DOCKER_CONTEXT")
            .env_remove("DOCKER_HOST");
        command::run(pool, job.id, build, "Build Docker falhou").await?;
    }
    db::advance(pool, job.id, Stage::Building, Stage::Starting).await?;
    let built = db::deployment(pool, job.id)
        .await?
        .context("Deploy ausente")?;
    let (container, port) = engine
        .start_image(
            job.id,
            input.container_port,
            built.image_tag.as_deref().context("Imagem ausente")?,
        )
        .await
        .context("Não foi possível iniciar o container")?;
    db::record_container(pool, job.id, &container, port).await?;
    db::advance(pool, job.id, Stage::Starting, Stage::CheckingHealth).await?;
    db::append_log(
        pool,
        job.id,
        "\n[worker] Verificando HTTP na porta local.\n",
    )
    .await?;
    health_check(engine, job.id, port).await?;
    db::append_log(
        pool,
        job.id,
        &format!("[worker] Instância saudável em http://127.0.0.1:{port}; preparando publicação\n"),
    )
    .await?;
    routing::publish(pool, routing, job, port).await?;
    let address = if let Some(routing) = routing {
        format!("http://{}.localhost:{}", app.name, routing.port)
    } else {
        format!("http://127.0.0.1:{port}")
    };
    db::append_log(
        pool,
        job.id,
        &format!("[worker] Aplicação pronta em {address}\n"),
    )
    .await?;
    if let Some(previous) = app.active_deployment_id
        && previous != job.id
        && engine.retire(previous).await.is_err()
    {
        db::append_log(
            pool,
            job.id,
            "[worker] Aviso: versão anterior precisa de limpeza de recursos.\n",
        )
        .await?;
    }
    tracing::info!(deployment = %job.id, port, "Deploy pronto");
    Ok(())
}

async fn clone_repository(pool: &PgPool, job: &Deployment, repository: &Path) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.file.allow=never",
            "clone",
            "--depth=1",
            "--no-tags",
            "--single-branch",
            "--",
            &job.repository_url,
        ])
        .arg(repository)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_COUNT", "0");
    command::run(
        pool,
        job.id,
        command,
        "Não foi possível baixar o repositório",
    )
    .await
}

async fn health_check(engine: &Engine, id: uuid::Uuid, port: u16) -> Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        if !engine.running(id).await? {
            bail!("Container encerrou antes de ficar saudável");
        }
        if let Ok(response) = client.get(format!("http://127.0.0.1:{port}/")).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("Aplicação não respondeu HTTP 2xx em / dentro de 30 segundos")
}
