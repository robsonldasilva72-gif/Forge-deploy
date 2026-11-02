//! PostgreSQL persistence. Migrations are embedded in the binary.
use chrono::{DateTime, Utc};
use forge_deploy_core::{CreateApplication, DeploymentStatus};
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;
mod management;
pub use management::*;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Application {
    pub id: Uuid,
    pub name: String,
    pub repository_url: String,
    pub container_port: i32,
    pub active_deployment_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub runtime_state: String,
    pub checked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Deployment {
    pub id: Uuid,
    pub application_id: Uuid,
    #[sqlx(default)]
    pub application_name: Option<String>,
    pub operation: String,
    pub source_deployment_id: Option<Uuid>,
    pub image_available: bool,
    pub status: String,
    pub repository_url: String,
    pub container_port: i32,
    pub commit_sha: Option<String>,
    pub image_tag: Option<String>,
    pub container_id: Option<String>,
    pub host_port: Option<i32>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn connect(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

pub async fn create_application(
    pool: &PgPool,
    input: &CreateApplication,
) -> Result<Application, sqlx::Error> {
    sqlx::query_as("INSERT INTO applications (id, name, repository_url, container_port) VALUES ($1, $2, $3, $4) RETURNING *")
        .bind(Uuid::new_v4()).bind(&input.name).bind(&input.repository_url)
        .bind(i32::from(input.container_port)).fetch_one(pool).await
}

pub async fn list_applications(pool: &PgPool) -> Result<Vec<Application>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM applications WHERE deleted_at IS NULL ORDER BY created_at DESC, id DESC LIMIT 200")
        .fetch_all(pool)
        .await
}

/// INSERT ... SELECT makes existence checking and enqueueing one atomic statement.
pub async fn enqueue(
    pool: &PgPool,
    application_id: Uuid,
) -> Result<Option<Deployment>, sqlx::Error> {
    sqlx::query_as("INSERT INTO deployments (id, application_id, repository_url, container_port) SELECT $1, id, repository_url, container_port FROM applications WHERE id = $2 AND deleted_at IS NULL RETURNING *")
        .bind(Uuid::new_v4()).bind(application_id).fetch_optional(pool).await
}

pub async fn deployment(pool: &PgPool, id: Uuid) -> Result<Option<Deployment>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM deployments WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn application_exists(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM applications WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn list_deployments(
    pool: &PgPool,
    application_id: Uuid,
) -> Result<Vec<Deployment>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM deployments WHERE application_id = $1 ORDER BY created_at DESC, id DESC LIMIT 200")
        .bind(application_id).fetch_all(pool).await
}

/// Claim exactly one queued row atomically. Execution is separately serialized by a session lock.
pub async fn claim(pool: &PgPool) -> Result<Option<Deployment>, sqlx::Error> {
    sqlx::query_as("WITH next AS (SELECT id FROM deployments WHERE status = 'queued' ORDER BY created_at, id FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE deployments SET status = 'fetching', started_at = now() FROM next WHERE deployments.id = next.id RETURNING deployments.*")
        .fetch_optional(pool).await
}

pub async fn advance(
    pool: &PgPool,
    id: Uuid,
    from: DeploymentStatus,
    to: DeploymentStatus,
) -> Result<(), sqlx::Error> {
    if !from.can_transition_to(to) || to == DeploymentStatus::Failed {
        return Err(sqlx::Error::Protocol(
            "invalid deployment transition".into(),
        ));
    }
    let result = sqlx::query("UPDATE deployments SET status = $3, finished_at = CASE WHEN $3 = 'ready' THEN now() ELSE finished_at END WHERE id = $1 AND status = $2")
        .bind(id).bind(from.as_str()).bind(to.as_str()).execute(pool).await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

pub async fn record_build(
    pool: &PgPool,
    id: Uuid,
    commit: &str,
    image: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE deployments SET commit_sha = $2, image_tag = $3 WHERE id = $1")
        .bind(id)
        .bind(commit)
        .bind(image)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_container(
    pool: &PgPool,
    id: Uuid,
    container: &str,
    port: u16,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE deployments SET container_id = $2, host_port = $3 WHERE id = $1")
        .bind(id)
        .bind(container)
        .bind(i32::from(port))
        .execute(pool)
        .await?;
    Ok(())
}

/// Keep only the last 64 KiB of UTF-8 text per deployment, with a truncation marker.
pub async fn append_log(pool: &PgPool, id: Uuid, text: &str) -> Result<(), sqlx::Error> {
    let text = text.replace('\0', "");
    sqlx::query("UPDATE deployments SET logs = CASE WHEN octet_length(logs || $2) > 65536 THEN '[older output truncated]\n' || right(logs || $2, 16000) ELSE logs || $2 END WHERE id = $1")
        .bind(id).bind(text).execute(pool).await?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DeploymentLog {
    pub logs: String,
    pub runtime_logs: String,
}

pub async fn logs(pool: &PgPool, id: Uuid) -> Result<Option<DeploymentLog>, sqlx::Error> {
    sqlx::query_as("SELECT logs, runtime_logs FROM deployments WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn interrupted(pool: &PgPool) -> Result<Vec<Deployment>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM deployments WHERE status IN ('fetching', 'building', 'starting', 'checking_health') ORDER BY created_at")
        .fetch_all(pool).await
}

pub async fn fail(pool: &PgPool, id: Uuid, reason: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE deployments SET status = 'failed', error = $2, host_port = NULL, finished_at = now() WHERE id = $1 AND status IN ('fetching', 'building', 'starting', 'checking_health')")
        .bind(id).bind(reason).execute(pool).await?;
    Ok(())
}
