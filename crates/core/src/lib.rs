//! Deployment domain and input validation, independent of HTTP and storage.
use serde::{Deserialize, Serialize};
use url::Url;

/// Observable stages of a deployment attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Queued,
    Fetching,
    Building,
    Starting,
    CheckingHealth,
    Ready,
    Failed,
}

impl DeploymentStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Fetching)
                | (Self::Fetching, Self::Building)
                | (Self::Building, Self::Starting)
                | (Self::Starting, Self::CheckingHealth)
                | (Self::CheckingHealth, Self::Ready)
        ) || (next == Self::Failed && !matches!(self, Self::Ready | Self::Failed))
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Fetching => "fetching",
            Self::Building => "building",
            Self::Starting => "starting",
            Self::CheckingHealth => "checking_health",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateApplication {
    pub name: String,
    pub repository_url: String,
    pub container_port: u16,
}

impl CreateApplication {
    /// Validate before persistence. Repository ownership/trust remains a user responsibility.
    pub fn validate(&self) -> Result<(), &'static str> {
        let name = self.name.as_bytes();
        if name.is_empty()
            || name.len() > 63
            || !name[0].is_ascii_lowercase()
            || !name.last().unwrap().is_ascii_alphanumeric()
            || !name
                .iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        {
            return Err(
                "O nome deve ter 1–63 caracteres, começar com letra minúscula, conter apenas letras minúsculas, números ou hífens e terminar com letra ou número",
            );
        }
        if self.container_port == 0 {
            return Err("A porta da aplicação deve estar entre 1 e 65535");
        }
        if self.repository_url.len() > 2048 {
            return Err("O endereço do repositório deve ter no máximo 2048 caracteres");
        }
        let url =
            Url::parse(&self.repository_url).map_err(|_| "Endereço do repositório inválido")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path().trim_matches('/').is_empty()
        {
            return Err(
                "O endereço do repositório deve ser uma URL HTTPS com caminho, sem credenciais, query ou fragmento",
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, repository_url: &str, container_port: u16) -> CreateApplication {
        CreateApplication {
            name: name.into(),
            repository_url: repository_url.into(),
            container_port,
        }
    }

    #[test]
    fn transitions_cannot_skip_build_or_revive_terminal_states() {
        use DeploymentStatus::*;
        assert!(Queued.can_transition_to(Fetching));
        assert!(CheckingHealth.can_transition_to(Ready));
        assert!(Building.can_transition_to(Failed));
        assert!(!Queued.can_transition_to(Ready));
        assert!(!Building.can_transition_to(Ready));
        assert!(!Failed.can_transition_to(Fetching));
        assert!(!Ready.can_transition_to(Failed));
    }

    #[test]
    fn accepts_valid_application() {
        assert!(
            input("minha-api", "https://github.com/example/api.git", 3000)
                .validate()
                .is_ok()
        );
        assert!(
            input("a", "https://github.com/example/api", 65535)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn rejects_invalid_names_and_ports() {
        for name in ["", "Api", "-api", "api-", "api_test", "1api", "api/path"] {
            assert!(
                input(name, "https://github.com/example/api", 3000)
                    .validate()
                    .is_err(),
                "{name}"
            );
        }
        assert!(
            input(&"a".repeat(64), "https://github.com/example/api", 3000)
                .validate()
                .is_err()
        );
        assert!(
            input("api", "https://github.com/example/api", 0)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn rejects_credentials_local_transports_and_ambiguous_urls() {
        for url in [
            "file:///etc/passwd",
            "git@github.com:example/api",
            "http://github.com/example/api",
            "https://user:secret@github.com/example/api",
            "https://github.com",
            "https://github.com/example/api?token=secret",
            "https://github.com/example/api#main",
        ] {
            assert!(input("api", url, 3000).validate().is_err(), "{url}");
        }
    }
}
