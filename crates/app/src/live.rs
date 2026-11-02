use super::*;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_core::Stream;
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};

async fn data(state: &AppState) -> Result<Value, ApiError> {
    let pool = state.database()?;
    Ok(
        json!({"applications":db::list_applications(pool).await?,"deployments":db::all_deployments(pool).await?,"worker_online":db::worker_online(pool).await?,"proxy_port":std::env::var("FORGE_PROXY_PORT").ok().and_then(|p|p.parse::<u16>().ok()).unwrap_or(8080)}),
    )
}

pub(super) async fn snapshot(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(data(&state).await?))
}

pub(super) async fn events(
    State(state): State<AppState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    state.database()?;
    let cancellation = state.shutdown.clone();
    let stream = async_stream::stream! {
        let mut previous=String::new();
        loop {
            match data(&state).await {
                Ok(value)=> {let encoded=value.to_string();if encoded!=previous {previous=encoded.clone();yield Ok(Event::default().data(encoded));}},
                Err(_)=> {yield Ok(Event::default().data("{\"unavailable\":true}"));break;}
            }
            tokio::select!{_=cancellation.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{}}
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}

pub(super) async fn logs(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let pool = state.database()?.clone();
    if db::deployment(&pool, id).await?.is_none() {
        return Err(ApiError::NotFound);
    }
    let cancellation = state.shutdown.clone();
    let stream = async_stream::stream! {
        let mut previous=String::new();
        loop {
            match (db::logs(&pool,id).await,db::deployment(&pool,id).await) {
                (Ok(Some(log)),Ok(Some(job)))=> {
                    let encoded=json!({"logs":log.logs,"runtime_logs":log.runtime_logs,"status":job.status,"error":job.error}).to_string();
                    if encoded!=previous{previous=encoded.clone();yield Ok(Event::default().data(encoded));}
                },
                _=>{yield Ok(Event::default().data("{\"unavailable\":true}"));break;}
            }
            tokio::select!{_=cancellation.cancelled()=>break,_=tokio::time::sleep(Duration::from_millis(500))=>{}}
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rollback {
    deployment_id: Uuid,
}
pub(super) async fn rollback(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<Rollback>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    let job = db::enqueue_action(state.database()?, id, "rollback", Some(input.deployment_id))
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}
pub(super) async fn action(
    State(state): State<AppState>,
    Path((id, action)): Path<(Uuid, String)>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    if !["stop", "delete", "cleanup"].contains(&action.as_str()) {
        return Err(ApiError::NotFound);
    }
    let job = db::enqueue_action(state.database()?, id, &action, None)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}
