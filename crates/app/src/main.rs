use anyhow::{Context, Result};
use forge_deploy_app::{AppState, router};
use forge_deploy_db as db;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let port: u16 = std::env::var("FORGE_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()
        .context("FORGE_PORT deve ser uma porta válida")?;
    let pool = match std::env::var("DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => {
            let pool = db::connect(&url).await.map_err(|_| anyhow::anyhow!("Não foi possível conectar ao PostgreSQL; confira DATABASE_URL e se o banco está ativo"))?;
            db::migrate(&pool)
                .await
                .context("Falha ao aplicar migrations")?;
            Some(pool)
        }
        _ => {
            tracing::warn!(
                "DATABASE_URL ausente: apenas liveness disponível; persistência responderá 503"
            );
            None
        }
    };
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(address = %listener.local_addr()?, "Forge Deploy; painel web e API; executor roda em processo separado");
    let state = AppState {
        pool,
        ..Default::default()
    };
    let cancellation = state.shutdown.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            shutdown().await;
            cancellation.cancel();
        })
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("Encerrando servidor");
}
