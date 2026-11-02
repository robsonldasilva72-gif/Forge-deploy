use anyhow::{Context, Result, bail};
use forge_deploy_db as db;
use forge_deploy_docker::Engine;
use forge_deploy_worker::{acquire_lock, monitor_lock, process, recover};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let once = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [arg] if arg == "--once" => true,
        _ => bail!("Uso: forge-deploy-worker [--once]"),
    };
    let url = std::env::var("DATABASE_URL")
        .context("Configure DATABASE_URL antes de iniciar o executor")?;
    let timeout: u64 = std::env::var("FORGE_DEPLOY_TIMEOUT_SECS")
        .unwrap_or_else(|_| "600".into())
        .parse()?;
    if timeout == 0 {
        bail!("FORGE_DEPLOY_TIMEOUT_SECS deve ser maior que zero");
    }
    let socket =
        std::env::var("FORGE_DOCKER_SOCKET").unwrap_or_else(|_| "/var/run/docker.sock".into());
    if !std::path::Path::new(&socket).is_absolute() {
        bail!("FORGE_DOCKER_SOCKET deve ser um caminho absoluto");
    }
    let engine = Engine::connect(socket).await?;
    let pool = db::connect(&url)
        .await
        .map_err(|_| anyhow::anyhow!("PostgreSQL indisponível; confira DATABASE_URL"))?;
    db::migrate(&pool).await?;
    let routing = forge_deploy_worker::routing::Routing::new(
        std::env::var("FORGE_CADDY_URL").unwrap_or_else(|_| "http://127.0.0.1:2019".into()),
        std::env::var("FORGE_PROXY_PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()?,
    )?;
    let mut lock = acquire_lock(&url).await?;
    let run = async {
        routing.sync(&pool, None).await?;
        recover(&pool, &engine).await?;
        loop {
            if let Some(job) = db::claim(&pool).await? {
                process(
                    &pool,
                    &engine,
                    &job,
                    Duration::from_secs(timeout),
                    Some(&routing),
                )
                .await?;
                if once {
                    break;
                }
            } else if once {
                break;
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        Ok::<(), anyhow::Error>(())
    };
    tracing::info!("Executor pronto; um deploy por vez");
    tokio::select! {
        result = run => result?,
        result = monitor_lock(&mut lock) => { result?; bail!("Monitor do lock encerrou"); },
        result=forge_deploy_worker::management::monitor_runtime(&pool,&engine)=> {result?;},
        result=forge_deploy_worker::routing::monitor(&pool,&routing)=>{result?;},
        _ = shutdown() => { tracing::info!("Executor interrompido; trabalho parcial será reconciliado no próximo início"); },
    }
    // Explicitly close: a pooled connection would retain a session advisory lock.
    sqlx::Connection::close(lock).await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
