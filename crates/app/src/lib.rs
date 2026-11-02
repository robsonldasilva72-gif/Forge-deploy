//! Local control API. The Leptos dashboard will be integrated in a later milestone.
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use forge_deploy_core::CreateApplication;
use forge_deploy_db::{self as db, Application, Deployment};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
mod live;
use axum::response::Html;
use tower_http::{compression::CompressionLayer, services::ServeDir};

#[derive(Clone, Default)]
pub struct AppState {
    pub pool: Option<PgPool>,
    pub shutdown: tokio_util::sync::CancellationToken,
}

impl AppState {
    fn database(&self) -> Result<&PgPool, ApiError> {
        self.pool.as_ref().ok_or(ApiError::Unavailable)
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../assets/index.html")) }),
        )
        .nest_service(
            "/assets",
            ServeDir::new(
                std::env::var("FORGE_ASSET_DIR")
                    .unwrap_or_else(|_| format!("{}/assets", env!("CARGO_MANIFEST_DIR"))),
            ),
        )
        .route("/api/snapshot", get(live::snapshot))
        .route("/api/events", get(live::events))
        .route(
            "/api/apps/{id}/rollback",
            axum::routing::post(live::rollback),
        )
        .route(
            "/api/apps/{id}/actions/{action}",
            axum::routing::post(live::action),
        )
        .route("/api/deployments/{id}/events", get(live::logs))
        .route(
            "/health/live",
            get(|| async { Json(json!({"status": "alive"})) }),
        )
        .route("/health/ready", get(readiness))
        .route("/api/apps", get(list_apps).post(create_app))
        .route(
            "/api/apps/{id}/deployments",
            get(list_deployments).post(enqueue),
        )
        .route("/api/deployments/{id}", get(deployment))
        .route("/api/deployments/{id}/logs", get(deployment_logs))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(CompressionLayer::new())
        .with_state(state)
}

#[derive(Debug)]
enum ApiError {
    Invalid(&'static str),
    NotFound,
    Conflict,
    Unavailable,
    Internal,
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            return Self::Conflict;
        }
        match error {
            sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_) => Self::Unavailable,
            _ => {
                // Avoid logging raw SQL errors: they can contain input values or credentials.
                tracing::error!("database operation failed");
                Self::Internal
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Invalid(message) => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_input", message),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "Recurso não encontrado"),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "name_conflict",
                "Já existe uma aplicação com esse nome",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "database_unavailable",
                "PostgreSQL indisponível; configure DATABASE_URL e inicie o banco",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Não foi possível concluir a operação",
            ),
        };
        (
            status,
            Json(json!({"error": {"code": code, "message": message}})),
        )
            .into_response()
    }
}

async fn readiness(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let pool = state.database()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await
    .map_err(|_| ApiError::Unavailable)?
    .map_err(|_| ApiError::Unavailable)?;
    Ok(Json(json!({"status": "ready", "database": "connected"})))
}

async fn list_apps(State(state): State<AppState>) -> Result<Json<Vec<Application>>, ApiError> {
    Ok(Json(db::list_applications(state.database()?).await?))
}

async fn create_app(
    State(state): State<AppState>,
    Json(input): Json<CreateApplication>,
) -> Result<(StatusCode, Json<Application>), ApiError> {
    input.validate().map_err(ApiError::Invalid)?;
    let app = db::create_application(state.database()?, &input).await?;
    Ok((StatusCode::CREATED, Json(app)))
}

async fn enqueue(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    let deployment = db::enqueue(state.database()?, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::ACCEPTED, Json(deployment)))
}

async fn deployment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Deployment>, ApiError> {
    Ok(Json(
        db::deployment(state.database()?, id)
            .await?
            .ok_or(ApiError::NotFound)?,
    ))
}

async fn list_deployments(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Deployment>>, ApiError> {
    let pool = state.database()?;
    if !db::application_exists(pool, id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(Json(db::list_deployments(pool, id).await?))
}

async fn deployment_logs(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<db::DeploymentLog>, ApiError> {
    Ok(Json(
        db::logs(state.database()?, id)
            .await?
            .ok_or(ApiError::NotFound)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    async fn call(method: &str, path: &str, body: &str) -> Response {
        router(AppState::default())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn liveness_is_independent_of_database_readiness() {
        assert_eq!(
            call("GET", "/health/live", "").await.status(),
            StatusCode::OK
        );
        assert_eq!(
            call("GET", "/health/ready", "").await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn unconfigured_database_is_reported_without_fake_results() {
        let response = call("GET", "/api/apps", "").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "database_unavailable");
    }

    #[tokio::test]
    async fn invalid_repository_is_rejected_before_storage() {
        let response = call(
            "POST",
            "/api/apps",
            r#"{"name":"api","repository_url":"file:///etc/passwd","container_port":3000}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn rejects_unknown_fields_and_oversized_requests() {
        assert_eq!(call("POST", "/api/apps", r#"{"name":"api","repository_url":"https://github.com/example/api","container_port":3000,"secret":"x"}"#).await.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            call("POST", "/api/apps", &"x".repeat(17000)).await.status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn valid_request_requires_database_and_ids_must_be_uuids() {
        assert_eq!(call("POST", "/api/apps", r#"{"name":"api","repository_url":"https://github.com/example/api","container_port":3000}"#).await.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            call("GET", "/api/deployments/not-an-id", "").await.status(),
            StatusCode::BAD_REQUEST
        );
    }
}
