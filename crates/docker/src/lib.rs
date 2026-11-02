//! Docker runtime operations. Only containers with matching ownership labels are removed.
use anyhow::{Context, Result, bail};
use bollard::{
    API_DEFAULT_VERSION, Docker,
    models::{ContainerCreateBody, HostConfig, HostConfigLogConfig, PortBinding},
    query_parameters::{
        CreateContainerOptionsBuilder, InspectContainerOptions, LogsOptionsBuilder,
        RemoveContainerOptionsBuilder, RemoveImageOptionsBuilder, StartContainerOptions,
        StopContainerOptionsBuilder,
    },
};
use futures_util::StreamExt;
use std::collections::HashMap;
use uuid::Uuid;

const OWNER: &str = "forge-deploy.deployment";

pub fn container_name(id: Uuid) -> String {
    format!("forge-deploy-{id}")
}

pub fn image_tag(id: Uuid) -> String {
    format!("forge-deploy:{id}")
}

#[derive(Clone)]
pub struct Engine {
    docker: Docker,
    pub socket: String,
}

impl Engine {
    pub async fn connect(socket: String) -> Result<Self> {
        let docker = Docker::connect_with_unix(&socket, 30, API_DEFAULT_VERSION)?;
        let docker = docker
            .negotiate_version()
            .await
            .context("Sem acesso ao Docker; verifique serviço e permissões do socket")?;
        docker.ping().await?;
        Ok(Self { docker, socket })
    }

    /// Bind an ephemeral host port to loopback only, with bounded memory and log rotation.
    pub async fn start(&self, id: Uuid, container_port: u16) -> Result<(String, u16)> {
        self.start_image(id, container_port, &image_tag(id)).await
    }

    pub async fn start_image(
        &self,
        id: Uuid,
        container_port: u16,
        image: &str,
    ) -> Result<(String, u16)> {
        let port_key = format!("{container_port}/tcp");
        let body = ContainerCreateBody {
            image: Some(image.into()),
            exposed_ports: Some(vec![port_key.clone()]),
            labels: Some(HashMap::from([(OWNER.to_owned(), id.to_string())])),
            host_config: Some(HostConfig {
                port_bindings: Some(HashMap::from([(
                    port_key.clone(),
                    Some(vec![PortBinding {
                        host_ip: Some("127.0.0.1".into()),
                        host_port: Some("0".into()),
                    }]),
                )])),
                memory: Some(256 * 1024 * 1024),
                memory_swap: Some(256 * 1024 * 1024),
                nano_cpus: Some(1_000_000_000),
                pids_limit: Some(128),
                cap_drop: Some(vec!["ALL".into()]),
                security_opt: Some(vec!["no-new-privileges:true".into()]),
                log_config: Some(HostConfigLogConfig {
                    typ: Some("json-file".into()),
                    config: Some(HashMap::from([
                        ("max-size".into(), "1m".into()),
                        ("max-file".into(), "2".into()),
                    ])),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let options = CreateContainerOptionsBuilder::new()
            .name(&container_name(id))
            .build();
        let created = self.docker.create_container(Some(options), body).await?;
        self.docker
            .start_container(&created.id, None::<StartContainerOptions>)
            .await?;
        let info = self
            .docker
            .inspect_container(&created.id, None::<InspectContainerOptions>)
            .await?;
        let port = info
            .network_settings
            .and_then(|s| s.ports)
            .and_then(|mut ports| ports.remove(&port_key).flatten())
            .and_then(|bindings| {
                bindings
                    .into_iter()
                    .find(|b| b.host_ip.as_deref() == Some("127.0.0.1"))
            })
            .and_then(|b| b.host_port)
            .context("Docker não retornou porta local")?
            .parse::<u16>()?;
        Ok((created.id, port))
    }

    pub async fn running(&self, id: Uuid) -> Result<bool> {
        let info = self
            .docker
            .inspect_container(&container_name(id), None::<InspectContainerOptions>)
            .await?;
        Ok(info.state.and_then(|state| state.running).unwrap_or(false))
    }

    pub async fn tail_logs(&self, id: Uuid) -> Result<String> {
        let options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .tail("100")
            .build();
        let name = container_name(id);
        let mut stream = self.docker.logs(&name, Some(options));
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            output.push_str(&chunk?.to_string());
            if output.len() > 16384 {
                let mut boundary = output.len() - 16384;
                while !output.is_char_boundary(boundary) {
                    boundary += 1;
                }
                output.drain(..boundary);
            }
        }
        Ok(output)
    }

    /// Idempotent cleanup, restricted to this deployment's labelled container.
    pub async fn cleanup(&self, id: Uuid) -> Result<()> {
        let name = container_name(id);
        let info = match self
            .docker
            .inspect_container(&name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => info,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let labels = info.config.and_then(|c| c.labels).unwrap_or_default();
        if labels.get(OWNER) != Some(&id.to_string()) {
            bail!("Recusando remover container sem o identificador esperado");
        }
        self.docker
            .remove_container(
                &name,
                Some(RemoveContainerOptionsBuilder::new().force(true).build()),
            )
            .await?;
        Ok(())
    }
    pub async fn retire(&self, id: Uuid) -> Result<()> {
        let name = container_name(id);
        let info = match self
            .docker
            .inspect_container(&name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => info,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if info
            .config
            .and_then(|c| c.labels)
            .unwrap_or_default()
            .get(OWNER)
            != Some(&id.to_string())
        {
            bail!("Container sem identificação esperada");
        }
        if info.state.and_then(|s| s.running) == Some(true) {
            self.docker
                .stop_container(&name, Some(StopContainerOptionsBuilder::new().t(5).build()))
                .await?;
        }
        self.cleanup(id).await
    }

    pub async fn image_exists(&self, image: &str) -> Result<bool> {
        match self.docker.inspect_image(image).await {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn remove_owned_image(&self, image: &str) -> Result<()> {
        let expected = Uuid::parse_str(
            image
                .strip_prefix("forge-deploy:")
                .context("Imagem fora do projeto")?,
        )?;
        let info = match self.docker.inspect_image(image).await {
            Ok(info) => info,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if info
            .config
            .and_then(|c| c.labels)
            .unwrap_or_default()
            .get(OWNER)
            != Some(&expected.to_string())
        {
            bail!("Imagem sem identificação esperada");
        }
        self.docker
            .remove_image(
                image,
                Some(RemoveImageOptionsBuilder::new().force(false).build()),
                None,
            )
            .await?;
        Ok(())
    }
}
