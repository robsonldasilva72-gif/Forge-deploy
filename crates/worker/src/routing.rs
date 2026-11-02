//! Caddy config and database cannot share a transaction: compensate and reconcile on uncertainty.
use anyhow::{Context, Result, bail};
use forge_deploy_db::{self as db, Deployment};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug)]
pub struct RoutingUncertain;
impl std::fmt::Display for RoutingUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Estado de publicação incerto; executor interrompido para preservar os containers e reconciliar no próximo início"
        )
    }
}
impl std::error::Error for RoutingUncertain {}

#[derive(Clone)]
pub struct Routing {
    client: reqwest::Client,
    admin: String,
    pub port: u16,
    server: String,
    gate: std::sync::Arc<tokio::sync::Mutex<()>>,
}
impl Routing {
    pub fn new(admin: String, port: u16) -> Result<Self> {
        let url = reqwest::Url::parse(&admin)?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || port == 0
        {
            bail!(
                "Admin Caddy deve ser HTTP local, sem credenciais; porta pública deve ser válida"
            );
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()?,
            admin: admin.trim_end_matches('/').into(),
            port,
            server: "forge".into(),
            gate: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        })
    }
    /// Allows isolated integration-test servers without overwriting the local workspace routes.
    pub fn with_server(mut self, server: String) -> Result<Self> {
        if server.is_empty()
            || !server
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            bail!("Identificador de servidor inválido");
        }
        self.server = server;
        Ok(self)
    }

    pub async fn sync(
        &self,
        pool: &PgPool,
        replacement: Option<(Uuid, Option<(Uuid, u16)>)>,
    ) -> Result<()> {
        let _guard = self.gate.lock().await;
        self.apply(pool, replacement).await
    }

    async fn apply(
        &self,
        pool: &PgPool,
        replacement: Option<(Uuid, Option<(Uuid, u16)>)>,
    ) -> Result<()> {
        let mut routes: Vec<Value> = Vec::new();
        for route in db::active_routes(pool).await? {
            if replacement.is_some_and(|(id, _)| id == route.id) {
                continue;
            }
            routes.push(route_json(&route.name, u16::try_from(route.host_port)?));
        }
        if let Some((app, Some((_deployment, port)))) = replacement {
            let app = db::application(pool, app)
                .await?
                .context("Aplicação não encontrada")?;
            routes.push(route_json(&app.name, port));
        }
        routes.push(json!({"handle":[{"handler":"static_response","status_code":404,"body":"Nenhuma aplicação publicada neste endereço."}],"terminal":true}));
        let config = json!({"listen":[format!("127.0.0.1:{}",self.port)],"routes":routes,"automatic_https":{"disable":true}});
        let endpoint = format!("{}/config/apps/http/servers/{}", self.admin, self.server);
        let current = self.client.get(&endpoint).send().await?;
        let missing = current.status() == reqwest::StatusCode::NOT_FOUND;
        if !missing {
            let current: Value = current.error_for_status()?.json().await?;
            if current == config {
                return Ok(());
            }
        }
        let request = if missing {
            self.client.put(&endpoint)
        } else {
            self.client.patch(&endpoint)
        };
        request
            .json(&config)
            .send()
            .await?
            .error_for_status()
            .context("Caddy recusou configuração")?;
        Ok(())
    }
}
fn route_json(name: &str, port: u16) -> Value {
    json!({"match":[{"host":[format!("{name}.localhost")]}],"handle":[{"handler":"reverse_proxy","upstreams":[{"dial":format!("127.0.0.1:{port}")}]}],"terminal":true})
}

pub async fn publish(
    pool: &PgPool,
    routing: Option<&Routing>,
    job: &Deployment,
    port: u16,
) -> Result<()> {
    let _guard = if let Some(routing) = routing {
        Some(routing.gate.lock().await)
    } else {
        None
    };
    if let Some(routing) = routing
        && let Err(error) = routing
            .apply(pool, Some((job.application_id, Some((job.id, port)))))
            .await
    {
        routing
            .apply(pool, None)
            .await
            .map_err(|_| RoutingUncertain)?;
        return Err(error);
    }
    if let Err(error) = db::activate(pool, job).await {
        // A commit can succeed even if its acknowledgement is lost.
        let app = db::application(pool, job.application_id)
            .await
            .map_err(|_| RoutingUncertain)?;
        let committed = app.is_some_and(|a| a.active_deployment_id == Some(job.id));
        if let Some(routing) = routing {
            routing
                .apply(pool, None)
                .await
                .map_err(|_| RoutingUncertain)?;
        }
        if !committed {
            return Err(error.into());
        }
    }
    Ok(())
}

pub async fn detach(pool: &PgPool, routing: Option<&Routing>, app: Uuid) -> Result<()> {
    let _guard = if let Some(routing) = routing {
        Some(routing.gate.lock().await)
    } else {
        None
    };
    if let Some(routing) = routing
        && let Err(error) = routing.apply(pool, Some((app, None))).await
    {
        routing
            .apply(pool, None)
            .await
            .map_err(|_| RoutingUncertain)?;
        return Err(error);
    }
    if let Err(error) = db::detach(pool, app, false).await {
        let current = db::application(pool, app)
            .await
            .map_err(|_| RoutingUncertain)?;
        let committed = current.is_some_and(|a| a.active_deployment_id.is_none());
        if let Some(routing) = routing {
            routing
                .apply(pool, None)
                .await
                .map_err(|_| RoutingUncertain)?;
        }
        if !committed {
            return Err(error.into());
        }
    }
    Ok(())
}

pub async fn monitor(pool: &PgPool, routing: &Routing) -> Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if routing.sync(pool, None).await.is_err() {
            tracing::warn!("Caddy indisponível; reconciliação será tentada novamente");
        }
    }
}
