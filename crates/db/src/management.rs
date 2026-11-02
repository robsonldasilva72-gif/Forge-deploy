use super::*;

pub async fn all_deployments(pool: &PgPool) -> Result<Vec<Deployment>, sqlx::Error> {
    sqlx::query_as("SELECT d.*, a.name AS application_name FROM deployments d JOIN applications a ON a.id = d.application_id ORDER BY d.created_at DESC, d.id DESC LIMIT 100").fetch_all(pool).await
}

pub async fn application(pool: &PgPool, id: Uuid) -> Result<Option<Application>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM applications WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn enqueue_action(
    pool: &PgPool,
    app: Uuid,
    operation: &str,
    source: Option<Uuid>,
) -> Result<Option<Deployment>, sqlx::Error> {
    if !["rollback", "stop", "delete", "cleanup"].contains(&operation) {
        return Err(sqlx::Error::Protocol("invalid action".into()));
    }
    sqlx::query_as("INSERT INTO deployments (id, application_id, repository_url, container_port, operation, source_deployment_id) SELECT $1, a.id, a.repository_url, a.container_port, $3, $4 FROM applications a WHERE a.id = $2 AND a.deleted_at IS NULL AND ($3 <> 'rollback' OR EXISTS(SELECT 1 FROM deployments d WHERE d.id = $4 AND d.application_id = a.id AND d.status = 'ready' AND d.operation IN ('deploy','rollback') AND d.image_available AND d.image_tag IS NOT NULL)) RETURNING *")
        .bind(Uuid::new_v4()).bind(app).bind(operation).bind(source).fetch_optional(pool).await
}

/// Commit the published version and terminal job state together.
pub async fn activate(pool: &PgPool, job: &Deployment) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query("UPDATE deployments SET status = 'ready', finished_at = now() WHERE id = $1 AND status = 'checking_health'").bind(job.id).execute(&mut *tx).await?;
    if row.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    let row = sqlx::query("UPDATE applications SET active_deployment_id = $2, runtime_state = 'running', checked_at = now() WHERE id = $1 AND deleted_at IS NULL").bind(job.application_id).bind(job.id).execute(&mut *tx).await?;
    if row.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    tx.commit().await
}

pub async fn detach(pool: &PgPool, app: Uuid, deleted: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE applications SET active_deployment_id = NULL, runtime_state = 'stopped', checked_at = now(), deleted_at = CASE WHEN $2 THEN now() ELSE deleted_at END WHERE id = $1").bind(app).bind(deleted).execute(pool).await?;
    Ok(())
}

pub async fn complete_action(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE deployments SET status = 'ready', finished_at = now() WHERE id = $1 AND status = 'fetching' AND operation IN ('stop','delete','cleanup')").bind(id).execute(pool).await?;
    Ok(())
}

pub async fn image_removed(pool: &PgPool, image: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE deployments SET image_available = FALSE WHERE image_tag = $1")
        .bind(image)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn runtime_state(
    pool: &PgPool,
    app: Uuid,
    deployment: Uuid,
    state: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE applications SET runtime_state = $3, checked_at = now() WHERE id = $1 AND active_deployment_id = $2").bind(app).bind(deployment).bind(state).execute(pool).await?;
    Ok(())
}

pub async fn worker_online(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM worker_heartbeat WHERE seen_at > now() - interval '8 seconds')").fetch_one(pool).await
}

pub async fn application_resources(
    pool: &PgPool,
    app: Uuid,
) -> Result<Vec<Deployment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM deployments WHERE application_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(app)
    .fetch_all(pool)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct ActiveRoute {
    pub id: Uuid,
    pub name: String,
    pub deployment_id: Uuid,
    pub host_port: i32,
}
pub async fn active_routes(pool: &PgPool) -> Result<Vec<ActiveRoute>, sqlx::Error> {
    sqlx::query_as("SELECT a.id, a.name, d.id AS deployment_id, d.host_port FROM applications a JOIN deployments d ON d.id = a.active_deployment_id WHERE a.deleted_at IS NULL AND d.host_port IS NOT NULL").fetch_all(pool).await
}

pub async fn runtime_logs(pool: &PgPool, id: Uuid, text: &str) -> Result<(), sqlx::Error> {
    let text = text.replace('\0', "");
    sqlx::query("UPDATE deployments SET runtime_logs=right($2,8000) WHERE id=$1 AND runtime_logs IS DISTINCT FROM right($2,8000)").bind(id).bind(text).execute(pool).await?;
    Ok(())
}
