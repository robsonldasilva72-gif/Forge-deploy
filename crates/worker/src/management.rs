use crate::routing::{self, Routing};
use anyhow::{Context, Result};
use forge_deploy_db::{self as db, Deployment};
use forge_deploy_docker::Engine;
use sqlx::PgPool;
use std::collections::HashSet;

pub async fn perform(
    pool: &PgPool,
    engine: &Engine,
    routing: Option<&Routing>,
    job: &Deployment,
) -> Result<()> {
    let app = db::application(pool, job.application_id)
        .await?
        .context("Aplicação foi removida")?;
    match job.operation.as_str() {
        "stop" | "delete" => {
            routing::detach(pool, routing, app.id).await?;
            for resource in db::application_resources(pool, app.id).await? {
                engine.retire(resource.id).await?;
            }
            if job.operation == "delete" {
                clean_images(pool, engine, job, true).await?;
                db::detach(pool, app.id, true).await?;
            }
        }
        "cleanup" => {
            for resource in db::application_resources(pool, app.id).await? {
                if app.active_deployment_id != Some(resource.id) {
                    engine.retire(resource.id).await?;
                }
            }
            clean_images(pool, engine, job, false).await?;
        }
        _ => anyhow::bail!("Operação desconhecida"),
    }
    db::append_log(pool, job.id, "[worker] Operação concluída.\n").await?;
    db::complete_action(pool, job.id).await?;
    Ok(())
}

async fn clean_images(pool: &PgPool, engine: &Engine, job: &Deployment, all: bool) -> Result<()> {
    let app = db::application(pool, job.application_id)
        .await?
        .context("Aplicação ausente")?;
    let resources = db::application_resources(pool, app.id).await?;
    let mut keep = HashSet::new();
    if !all {
        if let Some(active) = resources
            .iter()
            .find(|d| Some(d.id) == app.active_deployment_id)
            && let Some(image) = &active.image_tag
        {
            keep.insert(image.clone());
        }
        for resource in resources.iter().filter(|d| {
            d.status == "ready"
                && d.image_available
                && ["deploy", "rollback"].contains(&d.operation.as_str())
        }) {
            if keep.len() >= 3 {
                break;
            }
            if let Some(image) = &resource.image_tag {
                keep.insert(image.clone());
            }
        }
    }
    let images: HashSet<_> = resources
        .iter()
        .filter_map(|d| d.image_tag.as_ref())
        .cloned()
        .collect();
    for image in images.difference(&keep) {
        engine.remove_owned_image(image).await?;
        db::image_removed(pool, image).await?;
    }
    Ok(())
}

pub async fn monitor_runtime(pool: &PgPool, engine: &Engine) -> Result<()> {
    loop {
        for route in db::active_routes(pool).await? {
            let state = match engine.running(route.deployment_id).await {
                Ok(true) => "running",
                Ok(false) => "stopped",
                Err(_) => "unknown",
            };
            if let Ok(logs) = engine.tail_logs(route.deployment_id).await {
                db::runtime_logs(pool, route.deployment_id, &logs).await?;
            }
            db::runtime_state(pool, route.id, route.deployment_id, state).await?;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}
