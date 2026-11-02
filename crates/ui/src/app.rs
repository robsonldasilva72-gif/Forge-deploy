use leptos::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{EventSource, MessageEvent};

#[derive(Clone, Default, Deserialize)]
struct Snapshot {
    #[serde(default)]
    applications: Vec<Application>,
    #[serde(default)]
    deployments: Vec<Deployment>,
    #[serde(default)]
    worker_online: bool,
    #[serde(default = "proxy_port")]
    proxy_port: u16,
}
fn proxy_port() -> u16 {
    8080
}
#[derive(Clone, Deserialize)]
struct Application {
    id: String,
    name: String,
    repository_url: String,
    container_port: u16,
    active_deployment_id: Option<String>,
    runtime_state: String,
}
#[derive(Clone, Deserialize)]
struct Deployment {
    id: String,
    application_id: String,
    application_name: Option<String>,
    operation: String,
    status: String,
    commit_sha: Option<String>,
    image_tag: Option<String>,
    image_available: bool,
    error: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}
#[derive(Clone)]
enum Modal {
    Create,
    Confirm {
        app: String,
        action: String,
        source: Option<String>,
        title: String,
        description: String,
    },
}
#[derive(Clone, Copy)]
struct Ui {
    data: RwSignal<Snapshot>,
    connected: RwSignal<bool>,
    loaded: RwSignal<bool>,
    page: RwSignal<String>,
    app: RwSignal<String>,
    selected: RwSignal<Option<String>>,
    modal: RwSignal<Option<Modal>>,
    busy: RwSignal<bool>,
    notice: RwSignal<Option<(bool, String)>>,
}

fn active(status: &str) -> bool {
    matches!(
        status,
        "queued" | "fetching" | "building" | "starting" | "checking_health"
    )
}
fn status_label(status: &str) -> &'static str {
    match status {
        "queued" => "Na fila",
        "fetching" => "Preparando",
        "building" => "Construindo",
        "starting" => "Iniciando",
        "checking_health" => "Verificando",
        "ready" => "Concluído",
        "failed" => "Falhou",
        _ => "Desconhecido",
    }
}
fn operation_label(op: &str) -> &'static str {
    match op {
        "rollback" => "Rollback",
        "stop" => "Interrupção",
        "delete" => "Remoção",
        "cleanup" => "Limpeza",
        _ => "Deploy",
    }
}
fn commit(value: &Option<String>) -> String {
    value
        .as_ref()
        .map(|s| s.chars().take(7).collect())
        .unwrap_or_else(|| "—".into())
}
fn date(value: &str) -> String {
    js_sys::Date::new(&value.into())
        .to_locale_string("pt-BR", &wasm_bindgen::JsValue::UNDEFINED)
        .as_string()
        .unwrap_or_default()
}
fn duration(job: &Deployment) -> String {
    match (&job.started_at, &job.finished_at) {
        (Some(start), Some(end)) => format!(
            "{:.0}s",
            ((js_sys::Date::parse(end) - js_sys::Date::parse(start)) / 1000.).max(0.)
        ),
        _ if active(&job.status) => "Em andamento".into(),
        _ => "—".into(),
    }
}
fn repo(url: &str) -> String {
    url.strip_prefix("https://github.com/")
        .unwrap_or(url)
        .trim_end_matches(".git")
        .into()
}

fn subscribe(url: &str, callback: impl Fn(String) + 'static, on_error: impl Fn() + 'static) {
    let Ok(source) = EventSource::new(url) else {
        on_error();
        return;
    };
    let message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        if let Some(value) = event.data().as_string() {
            callback(value);
        }
    });
    let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| on_error());
    source.set_onmessage(Some(message.as_ref().unchecked_ref()));
    source.set_onerror(Some(error.as_ref().unchecked_ref()));
    let resources = send_wrapper::SendWrapper::new((source, message, error));
    on_cleanup(move || {
        resources.0.close();
        drop(resources);
    });
}

async fn post(url: &str, body: Value) -> Result<Value, String> {
    let response = gloo_net::http::Request::post(url)
        .json(&body)
        .map_err(|_| "Não foi possível preparar a solicitação".to_string())?
        .send()
        .await
        .map_err(|_| "Sem conexão com a API. Confira se o servidor está ativo.".to_string())?;
    let code = response.status();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&code) {
        let value: Value = serde_json::from_str(&text).unwrap_or_default();
        return Err(value["error"]["message"]
            .as_str()
            .unwrap_or("Solicitação recusada. Confira os campos e tente novamente.")
            .into());
    }
    serde_json::from_str(&text).map_err(|_| "Resposta inválida da API".into())
}
fn request(ui: Ui, path: String, body: Value) {
    ui.busy.set(true);
    ui.notice.set(None);
    leptos::task::spawn_local(async move {
        match post(&path, body).await {
            Ok(result) => {
                ui.modal.set(None);
                ui.selected.set(result["id"].as_str().map(str::to_string));
                ui.notice.set(Some((
                    true,
                    "Pedido recebido. Acompanhe cada etapa nos logs.".into(),
                )));
            }
            Err(error) => ui.notice.set(Some((false, error))),
        }
        ui.busy.set(false);
    });
}

#[component]
fn Icon(#[prop(default = "grid")] name: &'static str) -> impl IntoView {
    let path = match name {
        "apps" => "M4 4h6v6H4z M14 4h6v6h-6z M4 14h6v6H4z M14 14h6v6h-6z",
        "activity" => "M3 12h4l3-8 4 16 3-8h4",
        "book" => {
            "M4 4h6a3 3 0 0 1 2 1 3 3 0 0 1 2-1h6v15h-6a3 3 0 0 0-2 1 3 3 0 0 0-2-1H4z M12 5v15"
        }
        "plus" => "M12 5v14 M5 12h14",
        "arrow" => "M5 12h14 M13 6l6 6-6 6",
        "up" => "M12 19V5 M5 12l7-7 7 7",
        "external" => "M14 4h6v6 M20 4 10 14 M10 4H4v16h16v-6",
        "terminal" => "m4 6 6 6-6 6 M12 18h8",
        "close" => "m6 6 12 12 M6 18 18 6",
        "back" => "M19 12H5 m6-6-6 6 6 6",
        "clock" => "M12 7v5l3 2 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0",
        "settings" => "M4 7h16 M4 17h16 M8 4v6 M16 14v6",
        "check" => "m5 12 4 4 10-10",
        "git" => {
            "M8 6v10a3 3 0 0 0 6 0V8 M5 5a3 3 0 1 0 6 0 3 3 0 0 0-6 0 M11 7a3 3 0 1 0 6 0 3 3 0 0 0-6 0"
        }
        _ => "M3 3h7v7H3z M14 3h7v18h-7z M3 14h7v7H3z",
    };
    view! {
        <svg
            width={"18"}
            height={"18"}
            viewBox={"0 0 24 24"}
            fill={"none"}
            stroke={"currentColor"}
            stroke-width={"1.6"}
            stroke-linecap={"round"}
            stroke-linejoin={"round"}
            aria-hidden={"true"}
        >
            <path d={path} />
        </svg>
    }
}

#[component]
pub fn Dashboard() -> impl IntoView {
    let ui = Ui {
        data: RwSignal::new(Snapshot::default()),
        connected: RwSignal::new(false),
        loaded: RwSignal::new(false),
        page: RwSignal::new("overview".into()),
        app: RwSignal::new(String::new()),
        selected: RwSignal::new(None),
        modal: RwSignal::new(None),
        busy: RwSignal::new(false),
        notice: RwSignal::new(None),
    };
    subscribe(
        "/api/events",
        move |text| {
            if let Ok(snapshot) = serde_json::from_str::<Snapshot>(&text) {
                if serde_json::from_str::<Value>(&text)
                    .ok()
                    .is_some_and(|v| v["unavailable"] == true)
                {
                    ui.connected.set(false);
                    return;
                }
                ui.data.set(snapshot);
                ui.loaded.set(true);
                ui.connected.set(true);
            }
        },
        move || ui.connected.set(false),
    );
    let motion = RwSignal::new(storage("forge-motion").as_deref() != Some("off"));
    let workspace =
        RwSignal::new(storage("forge-workspace").unwrap_or_else(|| "Workspace pessoal".into()));
    let workspace_input = RwSignal::new(workspace.get_untracked());
    view! {
        <div class={move || if motion.get() { "dashboard" } else { "dashboard no-motion" }}>
            <div class={"landscape"} aria-hidden={"true"} />
            <div class={"shade"} aria-hidden={"true"} />
            <a class={"skip"} href={"#main"}>
                "Pular para o conteúdo"
            </a>
            <aside class={"sidebar"}>
                <a
                    class={"brand"}
                    aria-label={"Forge Deploy — início"}
                    href={"#"}
                    on:click={move |ev| {
                        ev.prevent_default();
                        ui.page.set("overview".into());
                        ui.app.set(String::new());
                    }}
                >
                    <img src={"/assets/mark.svg"} alt={""} />
                    <span>"forge"<span class={"brand-sub"}>"DEPLOY"</span></span>
                </a>
                <div class={"workspace-card"}>
                    <span class={"workspace-avatar"}>"P"</span>
                    <span>
                        <strong>{move || workspace.get()}</strong>
                        <small>"Ambiente de desenvolvimento"</small>
                    </span>
                </div>
                <div class={"nav-label"}>"WORKSPACE"</div>
                <nav aria-label={"Navegação principal"}>
                    {[
                        ("overview", "grid", "Visão geral"),
                        ("apps", "apps", "Aplicações"),
                        ("activity", "activity", "Atividade"),
                        ("learn", "book", "Como funciona"),
                    ]
                        .into_iter()
                        .map(|(page, icon, label)| {
                            view! {
                                <button
                                    aria-label={label}
                                    class={move || {
                                        if ui.page.get() == page {
                                            "nav-item active"
                                        } else {
                                            "nav-item"
                                        }
                                    }}
                                    aria-current={move || {
                                        if ui.page.get() == page { Some("page") } else { None }
                                    }}
                                    on:click={move |_| {
                                        ui.page.set(page.into());
                                        ui.app.set(String::new());
                                    }}
                                >
                                    <Icon name={icon} />
                                    <span>{label}</span>
                                    {(page == "apps")
                                        .then(|| {
                                            view! {
                                                <span class={"nav-count"}>
                                                    {move || ui.data.get().applications.len()}
                                                </span>
                                            }
                                        })}
                                </button>
                            }
                        })
                        .collect_view()}
                </nav>
                <div class={"sidebar-bottom"}>
                    <div class={"environment"}>
                        <div class={"environment-head"}>
                            <span class={move || {
                                if ui.data.get().worker_online { "dot online" } else { "dot" }
                            }} />
                            <strong>"Este computador"</strong>
                            <span class={"tiny-pill"}>"LOCAL"</span>
                        </div>
                        <p>
                            {move || {
                                if ui.data.get().worker_online {
                                    "Executor conectado"
                                } else {
                                    "Executor desconectado"
                                }
                            }}
                        </p>
                        <div class={"environment-line"}>
                            <span>"Rust + Docker"</span>
                            <span>"01 nó"</span>
                        </div>
                    </div>
                    <button
                        class={"nav-item"}
                        aria-label={"Preferências"}
                        on:click={move |_| ui.page.set("settings".into())}
                    >
                        <Icon name={"settings"} />
                        <span>"Preferências"</span>
                    </button>
                    <div class={"sidebar-foot"}>
                        "Construído para ir além."<span>"Forge Deploy · v0.1"</span>
                    </div>
                </div>
            </aside>
            <div class={"workspace-main"}>
                <header class={"topbar"}>
                    <div class={"breadcrumbs"}>
                        <span>"Workspace"</span>
                        <span>"/"</span>
                        <strong>
                            {move || match ui.page.get().as_str() {
                                "apps" => "Aplicações",
                                "activity" => "Atividade",
                                "learn" => "Como funciona",
                                "settings" => "Preferências",
                                _ => "Visão geral",
                            }}
                        </strong>
                    </div>
                    <div class={"topbar-right"}>
                        <span class={move || {
                            if ui.connected.get() {
                                "live-label"
                            } else {
                                "live-label disconnected"
                            }
                        }}>
                            <span class={"dot"} />
                            {move || {
                                if ui.connected.get() {
                                    "Atualização ao vivo"
                                } else {
                                    "Reconectando…"
                                }
                            }}
                        </span>
                        <span class={"profile"} title={"Workspace local"}>
                            "M"
                        </span>
                    </div>
                </header>
                <main id={"main"}>
                    <Show when={move || ui.loaded.get() && !ui.data.get().worker_online}>
                        <div class={"banner warning"}>
                            <Icon name={"clock"} />
                            <span>
                                "O executor está desconectado. Novos pedidos ficam na fila até ele voltar."
                            </span>
                        </div>
                    </Show>
                    <Show when={move || !ui.connected.get()}>
                        <div class={"banner"}>
                            <span class={"dot"} />
                            <span>
                                "Conectando ao ambiente local. Se não reconectar, confira a API e o PostgreSQL."
                            </span>
                        </div>
                    </Show>
                    {move || {
                        ui.notice
                            .get()
                            .map(|(success, text)| {
                                view! {
                                    <div
                                        role={"status"}
                                        class={if success {
                                            "banner success"
                                        } else {
                                            "banner error"
                                        }}
                                    >
                                        <Icon name={if success { "check" } else { "close" }} />
                                        <span>{text}</span>
                                        <button
                                            class={"icon-button"}
                                            aria-label={"Fechar aviso"}
                                            on:click={move |_| ui.notice.set(None)}
                                        >
                                            <Icon name={"close"} />
                                        </button>
                                    </div>
                                }
                            })
                    }}
                    {move || match ui.page.get().as_str() {
                        "learn" => view! { <Learn /> }.into_any(),
                        "settings" => {
                            view! {
                                <section class={"page-heading"}>
                                    <div class={"eyebrow"}>"DO SEU JEITO"</div>
                                    <h1>"Um espaço que é seu."</h1>
                                    <p>"Pequenos ajustes para trabalhar com mais clareza."</p>
                                </section>
                                <section class={"panel preferences"}>
                                    <h2>"Identidade do workspace"</h2>
                                    <label for={"workspace-name"}>"Nome do seu espaço"</label>
                                    <div class={"input-row"}>
                                        <input
                                            id={"workspace-name"}
                                            maxlength={"40"}
                                            prop:value={move || workspace_input.get()}
                                            on:input={move |ev| {
                                                workspace_input.set(event_target_value(&ev))
                                            }}
                                        />
                                        <button
                                            class={"primary"}
                                            on:click={move |_| {
                                                let name = workspace_input.get().trim().to_string();
                                                if !name.is_empty() {
                                                    save("forge-workspace", &name);
                                                    workspace.set(name);
                                                    ui.notice
                                                        .set(
                                                            Some((true, "Preferência salva neste navegador.".into())),
                                                        );
                                                }
                                            }}
                                        >
                                            "Salvar"
                                        </button>
                                    </div>
                                    <p class={"muted"}>
                                        "Esse nome é salvo apenas neste navegador."
                                    </p>
                                    <div class={"preference-row"}>
                                        <div>
                                            <h3>"Paisagem em movimento"</h3>
                                            <p>"Movimento lento e discreto na imagem de fundo."</p>
                                        </div>
                                        <button
                                            class={move || {
                                                if motion.get() { "toggle enabled" } else { "toggle" }
                                            }}
                                            role={"switch"}
                                            aria-checked={move || motion.get().to_string()}
                                            aria-label={"Movimento da paisagem"}
                                            on:click={move |_| {
                                                motion.update(|v| *v = !*v);
                                                save(
                                                    "forge-motion",
                                                    if motion.get() { "on" } else { "off" },
                                                );
                                            }}
                                        >
                                            <span />
                                        </button>
                                    </div>
                                    <p class={"muted"}>
                                        "A preferência de movimento reduzido do sistema é sempre respeitada."
                                    </p>
                                    <div class={"settings-note"}>
                                        <Icon name={"book"} />
                                        <span>
                                            "Paisagem gerada com IA, inspirada na Patagônia. Direção visual escolhida por você."
                                        </span>
                                    </div>
                                </section>
                            }
                                .into_any()
                        }
                        "apps" if !ui.app.get().is_empty() => {
                            view! { <ApplicationDetail ui={ui} /> }.into_any()
                        }
                        "activity" => {
                            view! {
                                <section class={"page-heading"}>
                                    <div class={"eyebrow"}>"CADA PASSO, À VISTA"</div>
                                    <h1>"Atividade"</h1>
                                    <p>
                                        "Os últimos 100 pedidos do seu ambiente, com status reais."
                                    </p>
                                </section>
                                <Activity ui={ui} limit={100} />
                            }
                                .into_any()
                        }
                        "apps" => {
                            view! {
                                <section class={"page-heading split"}>
                                    <div>
                                        <div class={"eyebrow"}>"UM ESPAÇO PARA CADA IDEIA"</div>
                                        <h1>"Aplicações"</h1>
                                        <p>"Do repositório à versão publicada."</p>
                                    </div>
                                    <button
                                        class={"primary"}
                                        on:click={move |_| ui.modal.set(Some(Modal::Create))}
                                    >
                                        <Icon name={"plus"} />
                                        "Nova aplicação"
                                    </button>
                                </section>
                                <Applications ui={ui} />
                            }
                                .into_any()
                        }
                        _ => {
                            view! {
                                <section class={"hero"}>
                                    <div class={"eyebrow"}>
                                        <span class={"mini-line"} />
                                        "SEU CÓDIGO. NOVOS HORIZONTES."
                                    </div>
                                    <h1>"Do código "<span>"ao ar."</span></h1>
                                    <p>
                                        "Publique, acompanhe e volte no tempo."<br />
                                        "Seu ambiente local, com cada etapa à vista."
                                    </p>
                                    <div class={"hero-actions"}>
                                        <button
                                            class={"primary"}
                                            on:click={move |_| ui.modal.set(Some(Modal::Create))}
                                        >
                                            <Icon name={"plus"} />
                                            "Nova aplicação"
                                        </button>
                                        <button
                                            class={"text-button"}
                                            on:click={move |_| ui.page.set("learn".into())}
                                        >
                                            "Entenda o fluxo"
                                            <Icon name={"arrow"} />
                                        </button>
                                    </div>
                                    <div class={"hero-caption"}>
                                        <span class={"caption-index"}>"01 /"</span>
                                        " Tudo começa com um repositório."
                                    </div>
                                </section>
                                <section class={"metrics"} aria-label={"Resumo do ambiente"}>
                                    <div class={"metric"}>
                                        <span>"Aplicações"<Icon name={"apps"} /></span>
                                        <strong>{move || ui.data.get().applications.len()}</strong>
                                        <small>"Ideias com um lugar para rodar"</small>
                                    </div>
                                    <div class={"metric"}>
                                        <span>"No ar"<span class={"dot online"} /></span>
                                        <strong>
                                            {move || {
                                                ui.data
                                                    .get()
                                                    .applications
                                                    .iter()
                                                    .filter(|a| {
                                                        a.active_deployment_id.is_some()
                                                            && a.runtime_state == "running"
                                                    })
                                                    .count()
                                            }}
                                        </strong>
                                        <small>"Containers verificados pelo executor"</small>
                                    </div>
                                    <div class={"metric"}>
                                        <span>"Em movimento"<Icon name={"activity"} /></span>
                                        <strong>
                                            {move || {
                                                ui.data
                                                    .get()
                                                    .deployments
                                                    .iter()
                                                    .filter(|d| active(&d.status))
                                                    .count()
                                            }}
                                        </strong>
                                        <small>"Pedidos na fila ou em execução"</small>
                                    </div>
                                </section>
                                <section class={"section"}>
                                    <div class={"section-heading"}>
                                        <div>
                                            <h2>
                                                "Suas aplicações"
                                                <span class={"count"}>
                                                    {move || ui.data.get().applications.len()}
                                                </span>
                                            </h2>
                                            <p>"Projetos diferentes. Um só horizonte."</p>
                                        </div>
                                        <button
                                            class={"text-button"}
                                            on:click={move |_| ui.page.set("apps".into())}
                                        >
                                            "Ver todas"
                                            <Icon name={"arrow"} />
                                        </button>
                                    </div>
                                    <Applications ui={ui} />
                                </section>
                                <section class={"section"}>
                                    <div class={"section-heading"}>
                                        <div>
                                            <h2>"Últimos movimentos"</h2>
                                            <p>"Cada publicação conta uma história."</p>
                                        </div>
                                        <button
                                            class={"text-button"}
                                            on:click={move |_| ui.page.set("activity".into())}
                                        >
                                            "Ver atividade"
                                            <Icon name={"arrow"} />
                                        </button>
                                    </div>
                                    <Activity ui={ui} limit={5} />
                                </section>
                            }
                                .into_any()
                        }
                    }}
                    <footer class={"main-footer"}>
                        <span>"Seu código. Sua infraestrutura. Seu ritmo."</span>
                        <span>"Feito em Rust · Executado localmente"</span>
                    </footer>
                </main>
            </div>
            {move || ui.modal.get().map(|modal| view! { <ModalView ui={ui} modal={modal} /> })}
            {move || ui.selected.get().map(|id| view! { <DeployDrawer ui={ui} id={id} /> })}
        </div>
    }
}

#[component]
fn Applications(ui: Ui) -> impl IntoView {
    view! {
        <div class={"app-grid"}>
            {move || {
                ui.data
                    .get()
                    .applications
                    .into_iter()
                    .map(|app| view! { <AppCard ui={ui} app={app} /> })
                    .collect_view()
            }}<button class={"new-card"} on:click={move |_| ui.modal.set(Some(Modal::Create))}>
                <span class={"new-card-icon"}>
                    <Icon name={"plus"} />
                </span>
                <strong>"Seu próximo projeto"</strong>
                <small>"Conecte um repositório e comece."</small>
            </button>
        </div>
    }
}

#[component]
fn AppCard(ui: Ui, app: Application) -> impl IntoView {
    let app = StoredValue::new(app);
    let id = app.with_value(|a| a.id.clone());
    let id_publish = id.clone();
    let id_detail = id.clone();
    let active_id = app.with_value(|a| a.active_deployment_id.clone());
    let latest = ui.data.with_untracked(|s| {
        s.deployments
            .iter()
            .find(|d| d.application_id == id)
            .cloned()
    });
    let running =
        app.with_value(|a| a.active_deployment_id.is_some() && a.runtime_state == "running");
    let pending = ui.data.with_untracked(|s| {
        s.deployments
            .iter()
            .any(|d| d.application_id == id && active(&d.status))
    });
    let label = if pending {
        "Em andamento"
    } else if running {
        "No ar"
    } else if active_id.is_some() {
        "Verificando"
    } else {
        "Sem versão ativa"
    };
    let url = app.with_value(|a| {
        format!(
            "http://{}.localhost:{}",
            a.name,
            ui.data.get_untracked().proxy_port
        )
    });
    view! {
        <article class={"app-card"}>
            <div class={"app-card-head"}>
                <span class={"app-avatar"}>
                    <Icon name={"terminal"} />
                </span>
                <span class={if running {
                    "badge ready"
                } else if pending {
                    "badge building"
                } else {
                    "badge"
                }}>
                    <span class={"dot"} />
                    {label}
                </span>
            </div>
            <button
                class={"app-title"}
                on:click={move |_| {
                    ui.app.set(id_detail.clone());
                    ui.page.set("apps".into());
                }}
            >
                {app.with_value(|a| a.name.clone())}
            </button>
            <div class={"repo"}>
                <Icon name={"git"} />
                <span>{app.with_value(|a| repo(&a.repository_url))}</span>
            </div>
            <div class={"app-version"}>
                <span>"Última atividade"</span>
                <code>
                    {latest.as_ref().map(|d| commit(&d.commit_sha)).unwrap_or_else(|| "—".into())}
                </code>
            </div>
            <div class={"app-card-bottom"}>
                <button
                    class={"deploy-button"}
                    disabled={move || ui.busy.get() || pending}
                    on:click={move |_| request(
                        ui,
                        format!("/api/apps/{id_publish}/deployments"),
                        json!({}),
                    )}
                >
                    <Icon name={"up"} />
                    "Publicar"
                </button>
                {active_id
                    .map(|_| {
                        view! {
                            <a
                                class={"icon-button"}
                                href={url}
                                target={"_blank"}
                                rel={"noopener noreferrer"}
                                aria-label={"Abrir aplicação publicada"}
                            >
                                <Icon name={"external"} />
                            </a>
                        }
                    })}
            </div>
        </article>
    }
}

#[component]
fn Activity(ui: Ui, limit: usize) -> impl IntoView {
    view! {
        <div class={"panel activity-panel"}>
            <div class={"activity-header"}>
                <span>"APLICAÇÃO / OPERAÇÃO"</span>
                <span>"STATUS"</span>
                <span>"VERSÃO"</span>
                <span>"QUANDO"</span>
                <span />
            </div>
            {move || {
                let snapshot = ui.data.get();
                if snapshot.deployments.is_empty() {
                    return view! {
                        <div class={"empty"}>
                            <Icon name={"activity"} />
                            <h3>"Toda história tem um primeiro deploy."</h3>
                            <p>"Suas publicações e operações vão aparecer aqui."</p>
                        </div>
                    }
                        .into_any();
                }
                snapshot
                    .deployments
                    .into_iter()
                    .take(limit)
                    .map(|job| {
                        let name = snapshot
                            .applications
                            .iter()
                            .find(|a| a.id == job.application_id)
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| {
                                job.application_name
                                    .clone()
                                    .unwrap_or_else(|| "Aplicação removida".into())
                            });
                        let id = job.id.clone();
                        view! {
                            <button
                                class={"activity-row"}
                                on:click={move |_| ui.selected.set(Some(id.clone()))}
                            >
                                <span class={"activity-name"}>
                                    <span class={"row-icon"}>
                                        <Icon name={if job.operation == "deploy" {
                                            "up"
                                        } else {
                                            "clock"
                                        }} />
                                    </span>
                                    <span>
                                        <strong>{name}</strong>
                                        <small>{operation_label(&job.operation)}</small>
                                    </span>
                                </span>
                                <span>
                                    <span class={format!("badge {}", job.status)}>
                                        <span class={"dot"} />
                                        {status_label(&job.status)}
                                    </span>
                                </span>
                                <code>{commit(&job.commit_sha)}</code>
                                <span class={"time"}>{date(&job.created_at)}</span>
                                <Icon name={"arrow"} />
                            </button>
                        }
                    })
                    .collect_view()
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn ApplicationDetail(ui: Ui) -> impl IntoView {
    view! {
        <button class={"text-button back-link"} on:click={move |_| ui.app.set(String::new())}>
            <Icon name={"back"} />
            "Todas as aplicações"
        </button>
        {move || {
            let data = ui.data.get();
            let Some(app) = data.applications.iter().find(|a| a.id == ui.app.get()).cloned() else {
                return view! { <div class={"empty"}>"Aplicação não encontrada."</div> }
                    .into_any();
            };
            let jobs: Vec<_> = data
                .deployments
                .iter()
                .filter(|d| d.application_id == app.id)
                .cloned()
                .collect();
            let app_id = app.id.clone();
            let stop_id = app.id.clone();
            let clean_id = app.id.clone();
            let delete_id = app.id.clone();
            let has_active = app.active_deployment_id.is_some();
            let pending = jobs.iter().any(|d| active(&d.status));
            let active_job = jobs
                .iter()
                .find(|d| Some(&d.id) == app.active_deployment_id.as_ref())
                .cloned();
            view! {
                <section class={"page-heading split"}>
                    <div>
                        <div class={"eyebrow"}>"SUA APLICAÇÃO"</div>
                        <h1>{app.name.clone()}</h1>
                        <p>{repo(&app.repository_url)}</p>
                    </div>
                    <button
                        class={"primary"}
                        disabled={move || ui.busy.get() || pending}
                        on:click={move |_| request(
                            ui,
                            format!("/api/apps/{app_id}/deployments"),
                            json!({}),
                        )}
                    >
                        <Icon name={"up"} />
                        "Publicar versão"
                    </button>
                </section>
                <div class={"detail-summary panel"}>
                    <div>
                        <span class={"eyebrow"}>"ENDEREÇO LOCAL"</span>
                        <a
                            href={format!("http://{}.localhost:{}", app.name, data.proxy_port)}
                            target={"_blank"}
                            rel={"noopener noreferrer"}
                        >
                            {format!("{}.localhost:{}", app.name, data.proxy_port)}
                            <Icon name={"external"} />
                        </a>
                    </div>
                    <div>
                        <span class={"eyebrow"}>"VERSÃO ATIVA"</span>
                        <code>
                            {active_job
                                .as_ref()
                                .map(|d| commit(&d.commit_sha))
                                .unwrap_or_else(|| "Nenhuma".into())}
                        </code>
                    </div>
                    <div>
                        <span class={"eyebrow"}>"PORTA DO CONTAINER"</span>
                        <code>{app.container_port}</code>
                    </div>
                </div>
                <div class={"action-bar"}>
                    <span>"Gerenciar ambiente"</span>
                    <button
                        class={"secondary"}
                        disabled={move || ui.busy.get() || pending || !has_active}
                        on:click={move |_| {
                            ui.modal
                                .set(
                                    Some(Modal::Confirm {
                                        app: stop_id.clone(),
                                        action: "stop".into(),
                                        source: None,
                                        title: "Interromper aplicação?".into(),
                                        description: "A rota será desativada e os containers serão removidos. As imagens permanecem disponíveis para restaurar uma versão."
                                            .into(),
                                    }),
                                )
                        }}
                    >
                        "Interromper"
                    </button>
                    <button
                        class={"secondary"}
                        disabled={move || ui.busy.get() || pending}
                        on:click={move |_| {
                            ui.modal
                                .set(
                                    Some(Modal::Confirm {
                                        app: clean_id.clone(),
                                        action: "cleanup".into(),
                                        source: None,
                                        title: "Limpar recursos antigos?".into(),
                                        description: "Mantém a imagem ativa e até duas outras imagens recentes. Remove containers inativos e imagens mais antigas desta aplicação. As versões removidas deixam de permitir rollback."
                                            .into(),
                                    }),
                                )
                        }}
                    >
                        "Limpar versões antigas"
                    </button>
                    <button
                        class={"danger-text"}
                        disabled={move || ui.busy.get() || pending}
                        on:click={move |_| {
                            ui.modal
                                .set(
                                    Some(Modal::Confirm {
                                        app: delete_id.clone(),
                                        action: "delete".into(),
                                        source: None,
                                        title: "Remover aplicação?".into(),
                                        description: "Remove a rota, os containers e as imagens gerenciadas desta aplicação. O repositório Git original não é apagado."
                                            .into(),
                                    }),
                                )
                        }}
                    >
                        "Remover"
                    </button>
                </div>
                <div class={"section-heading"}>
                    <div>
                        <h2>"Versões e operações"</h2>
                        <p>"Uma imagem guardada é um caminho de volta."</p>
                    </div>
                </div>
                <div class={"panel versions"}>
                    {jobs
                        .into_iter()
                        .map(|job| {
                            let id = job.id.clone();
                            let rollback_id = job.id.clone();
                            let app_id = job.application_id.clone();
                            let is_active = app.active_deployment_id.as_ref() == Some(&job.id);
                            let can_restore = job.status == "ready" && job.image_available
                                && job.image_tag.is_some() && !is_active;
                            view! {
                                <div class={"version-row"}>
                                    <button
                                        class={"version-info"}
                                        on:click={move |_| ui.selected.set(Some(id.clone()))}
                                    >
                                        <span class={format!(
                                            "badge {}",
                                            job.status,
                                        )}>{status_label(&job.status)}</span>
                                        <strong>
                                            {operation_label(&job.operation)}" "
                                            <code>{commit(&job.commit_sha)}</code>
                                        </strong>
                                        <small>{date(&job.created_at)}</small>
                                    </button>
                                    {if is_active {
                                        view! {
                                            <span class={"active-version"}>
                                                <span class={"dot online"} />
                                                "Versão ativa"
                                            </span>
                                        }
                                            .into_any()
                                    } else if can_restore {
                                        view! {
                                            <button
                                                class={"secondary"}
                                                disabled={move || ui.busy.get() || pending}
                                                on:click={move |_| {
                                                    ui.modal
                                                        .set(
                                                            Some(Modal::Confirm {
                                                                app: app_id.clone(),
                                                                action: "rollback".into(),
                                                                source: Some(rollback_id.clone()),
                                                                title: "Restaurar esta versão?".into(),
                                                                description: "Uma nova instância será iniciada a partir da imagem existente, sem rebuild. O tráfego só muda depois da verificação HTTP. O banco de dados da aplicação não é revertido."
                                                                    .into(),
                                                            }),
                                                        )
                                                }}
                                            >
                                                "Restaurar"
                                            </button>
                                        }
                                            .into_any()
                                    } else {
                                        view! { <span class={"muted"}>{duration(&job)}</span> }
                                            .into_any()
                                    }}
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
            }
                .into_any()
        }}
    }
}

#[component]
fn ModalView(ui: Ui, modal: Modal) -> impl IntoView {
    ui.notice.set(None);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    Effect::new(move |_| {
        if let Some(dialog) = dialog.get() {
            let _ = dialog.show_modal();
        }
    });
    let name = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let port = RwSignal::new("3000".to_string());
    view! {
        <dialog
            node_ref={dialog}
            class={"modal"}
            on:cancel={move |ev: web_sys::Event| {
                ev.prevent_default();
                if !ui.busy.get() {
                    ui.modal.set(None);
                }
            }}
        >
            <button
                class={"icon-button modal-close"}
                aria-label={"Fechar"}
                disabled={move || ui.busy.get()}
                on:click={move |_| ui.modal.set(None)}
            >
                <Icon name={"close"} />
            </button>
            {match modal {
                Modal::Create => {
                    view! {
                        <div class={"eyebrow"}>"UM NOVO HORIZONTE"</div>
                        <h2>"Dê um lugar à sua ideia."</h2>
                        <p>
                            "Conecte um repositório público com Dockerfile. O primeiro deploy começa quando você publicar."
                        </p>
                        <form on:submit={move |ev| {
                            ev.prevent_default();
                            ui.busy.set(true);
                            ui.notice.set(None);
                            let body = json!(
                                {"name":name.get(),"repository_url":repository.get(),"container_port":port.get().parse::<u16>().unwrap_or(0)}
                            );
                            leptos::task::spawn_local(async move {
                                match post("/api/apps", body).await {
                                    Ok(app) => {
                                        ui.app.set(app["id"].as_str().unwrap_or_default().into());
                                        ui.page.set("apps".into());
                                        ui.modal.set(None);
                                        ui.notice
                                            .set(
                                                Some((
                                                    true,
                                                    "Aplicação cadastrada. Publique sua primeira versão."
                                                        .into(),
                                                )),
                                            );
                                    }
                                    Err(error) => ui.notice.set(Some((false, error))),
                                };
                                ui.busy.set(false);
                            });
                        }}>
                            <label for={"app-name"}>"Nome da aplicação"</label>
                            <input
                                id={"app-name"}
                                autofocus
                                required
                                maxlength={"63"}
                                pattern={"[a-z](([a-z0-9]|-)*[a-z0-9])?"}
                                placeholder={"minha-aplicacao"}
                                prop:value={move || name.get()}
                                on:input={move |ev| name.set(event_target_value(&ev))}
                            />
                            <small>
                                "Letras minúsculas, números e hífens. Será seu endereço local."
                            </small>
                            <label for={"repo-url"}>"Repositório público"</label>
                            <input
                                id={"repo-url"}
                                type={"url"}
                                required
                                placeholder={"https://github.com/voce/seu-projeto"}
                                prop:value={move || repository.get()}
                                on:input={move |ev| repository.set(event_target_value(&ev))}
                            />
                            <label for={"container-port"}>"Porta da aplicação"</label>
                            <input
                                id={"container-port"}
                                type={"number"}
                                min={"1"}
                                max={"65535"}
                                required
                                prop:value={move || port.get()}
                                on:input={move |ev| port.set(event_target_value(&ev))}
                            />
                            <small>
                                "A porta em que sua aplicação escuta dentro do container."
                            </small>
                            <div class={"modal-note"}>
                                <Icon name={"book"} />
                                <span>
                                    "Use seus próprios repositórios ou código confiável. Os builds executam no seu computador."
                                </span>
                            </div>
                            <button
                                class={"primary full"}
                                type={"submit"}
                                disabled={move || ui.busy.get()}
                            >
                                {move || {
                                    if ui.busy.get() {
                                        "Cadastrando…"
                                    } else {
                                        "Criar aplicação"
                                    }
                                }}
                                <Icon name={"arrow"} />
                            </button>
                        </form>
                    }
                        .into_any()
                }
                Modal::Confirm { app, action, source, title, description } => {
                    view! {
                        <div class={"eyebrow"}>"VOCÊ NO CONTROLE"</div>
                        <h2>{title}</h2>
                        <p>{description}</p>
                        <div class={"modal-actions"}>
                            <button
                                class={"secondary"}
                                disabled={move || ui.busy.get()}
                                on:click={move |_| ui.modal.set(None)}
                            >
                                "Cancelar"
                            </button>
                            <button
                                class={"primary"}
                                disabled={move || ui.busy.get()}
                                on:click={move |_| {
                                    let path = if action == "rollback" {
                                        format!("/api/apps/{app}/rollback")
                                    } else {
                                        format!("/api/apps/{app}/actions/{action}")
                                    };
                                    request(ui, path, json!({"deployment_id":source}));
                                }}
                            >
                                {move || if ui.busy.get() { "Enviando…" } else { "Confirmar" }}
                            </button>
                        </div>
                    }
                        .into_any()
                }
            }}
            {move || {
                ui.notice
                    .get()
                    .filter(|(ok, _)| !*ok)
                    .map(|(_, text)| {
                        view! {
                            <p class={"form-error"} role={"alert"}>
                                {text}
                            </p>
                        }
                    })
            }}
        </dialog>
    }
}

#[component]
fn DeployDrawer(ui: Ui, id: String) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    Effect::new(move |_| {
        if let Some(dialog) = dialog.get() {
            let _ = dialog.show_modal();
        }
    });
    let logs = RwSignal::new("Conectando aos logs…".to_string());
    let runtime = RwSignal::new("Aguardando saída da aplicação…".to_string());
    let channel = RwSignal::new(false);
    let stream_ok = RwSignal::new(false);
    let follow = RwSignal::new(true);
    let terminal = NodeRef::<leptos::html::Pre>::new();
    Effect::new(move |_| {
        let _ = logs.get();
        let _ = runtime.get();
        let _ = channel.get();
        if follow.get()
            && let Some(pre) = terminal.get()
        {
            pre.set_scroll_top(pre.scroll_height());
        }
    });
    subscribe(
        &format!("/api/deployments/{id}/events"),
        move |text| {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                if let Some(text) = value["runtime_logs"].as_str() {
                    runtime.set(if text.is_empty() {
                        "Nenhuma saída recente da aplicação.".into()
                    } else {
                        text.into()
                    });
                }
                if let Some(text) = value["logs"].as_str() {
                    logs.set(if text.is_empty() {
                        "Aguardando o executor…".into()
                    } else {
                        text.into()
                    });
                    stream_ok.set(true);
                } else {
                    stream_ok.set(false);
                }
            }
        },
        move || stream_ok.set(false),
    );
    let selected = id.clone();
    view! {
        <dialog
            node_ref={dialog}
            class={"deploy-drawer"}
            aria-label={"Detalhes do deploy"}
            on:cancel={move |ev: web_sys::Event| {
                ev.prevent_default();
                ui.selected.set(None);
            }}
        >
            <div class={"drawer-heading"}>
                <span class={"eyebrow"}>"CADA ETAPA, À VISTA"</span>
                <button
                    class={"icon-button"}
                    aria-label={"Fechar detalhes"}
                    on:click={move |_| ui.selected.set(None)}
                >
                    <Icon name={"close"} />
                </button>
            </div>
            {move || {
                let snapshot = ui.data.get();
                let Some(job) = snapshot.deployments.iter().find(|d| d.id == selected).cloned()
                else {
                    return view! { <h2>"Carregando pedido…"</h2> }.into_any();
                };
                let name = snapshot
                    .applications
                    .iter()
                    .find(|a| a.id == job.application_id)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| {
                        job.application_name
                            .clone()
                            .unwrap_or_else(|| "Aplicação removida".into())
                    });
                let stages = if job.operation == "rollback" {
                    ["Origem", "Imagem", "Container", "Verificação", "Pronto"]
                } else {
                    ["Código", "Build", "Container", "Verificação", "Pronto"]
                };
                let step = match job.status.as_str() {
                    "fetching" => 1,
                    "building" => 2,
                    "starting" => 3,
                    "checking_health" => 4,
                    "ready" => 5,
                    _ => 0,
                };
                view! {
                    <h2>{name}</h2>
                    <div class={"drawer-meta"}>
                        <span class={format!("badge {}", job.status)}>
                            <span class={"dot"} />
                            {status_label(&job.status)}
                        </span>
                        <span>{operation_label(&job.operation)}</span>
                        <code>{commit(&job.commit_sha)}</code>
                        <span>{duration(&job)}</span>
                    </div>
                    {["deploy", "rollback"]
                        .contains(&job.operation.as_str())
                        .then(|| {
                            view! {
                                <div class={"pipeline"}>
                                    {stages
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, label)| {
                                            view! {
                                                <div class={if step > i {
                                                    "pipeline-step passed"
                                                } else {
                                                    "pipeline-step"
                                                }}>
                                                    <span>
                                                        {if step > i { "✓".into() } else { format!("{}", i + 1) }}
                                                    </span>
                                                    <small>{label}</small>
                                                </div>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                        })}
                    {job
                        .error
                        .clone()
                        .map(|error| view! { <div class={"banner error"}>{error}</div> })}
                    {snapshot
                        .applications
                        .iter()
                        .find(|a| a.active_deployment_id.as_ref() == Some(&job.id))
                        .map(|app| {
                            view! {
                                <a
                                    class={"published-link"}
                                    href={format!(
                                        "http://{}.localhost:{}",
                                        app.name,
                                        snapshot.proxy_port,
                                    )}
                                    target={"_blank"}
                                    rel={"noopener noreferrer"}
                                >
                                    "Abrir aplicação publicada"
                                    <Icon name={"external"} />
                                </a>
                            }
                        })}
                }
                    .into_any()
            }}
            <div class={"log-tabs"}>
                <button
                    class={move || if !channel.get() { "selected" } else { "" }}
                    on:click={move |_| channel.set(false)}
                >
                    "Build e deploy"
                </button>
                <button
                    class={move || if channel.get() { "selected" } else { "" }}
                    on:click={move |_| channel.set(true)}
                >
                    "Aplicação"
                </button>
            </div>
            <div class={"terminal-heading"}>
                <span>
                    <Icon name={"terminal"} />
                    {move || {
                        if channel.get() { "Saída da aplicação" } else { "Saída do executor" }
                    }}
                </span>
                <div class={"terminal-controls"}>
                    <button
                        aria-pressed={move || follow.get().to_string()}
                        on:click={move |_| follow.update(|v| *v = !*v)}
                    >
                        {move || if follow.get() { "✓ Seguir saída" } else { "Seguir saída" }}
                    </button>
                    <small>
                        {move || if stream_ok.get() { "AO VIVO" } else { "RECONECTANDO" }}
                    </small>
                </div>
            </div>
            <pre node_ref={terminal} class={"terminal"} tabindex={"0"}>
                {move || if channel.get() { runtime.get() } else { logs.get() }}
            </pre>
            <p class={"drawer-footnote"}>
                "Os logs preservam a saída recente do deploy. O estado pronto indica que a verificação HTTP passou."
            </p>
        </dialog>
    }
}

#[component]
fn Learn() -> impl IntoView {
    view! {
        <section class={"page-heading"}>
            <div class={"eyebrow"}>"CLAREZA EM CADA CAMADA"</div>
            <h1>"Uma ideia. Cinco etapas."</h1>
            <p>"Você entrega o código. A plataforma organiza o caminho até ele estar no ar."</p>
        </section>
        <div class={"learn-grid"}>
            {[
                (
                    "01",
                    "Conecte seu código",
                    "Cadastre um repositório público com Dockerfile e a porta usada pela aplicação.",
                ),
                (
                    "02",
                    "Peça uma publicação",
                    "O pedido entra em uma fila persistente. O executor trabalha em uma versão por vez.",
                ),
                (
                    "03",
                    "Construa uma imagem",
                    "O código é baixado e o Dockerfile vira uma imagem identificada pelo deploy e pelo commit.",
                ),
                (
                    "04",
                    "Verifique antes de trocar",
                    "Um novo container precisa responder HTTP antes de receber o tráfego do endereço local.",
                ),
                (
                    "05",
                    "Avance com um caminho de volta",
                    "O Caddy encaminha o tráfego para a versão aprovada. Imagens anteriores podem ser restauradas sem rebuild.",
                ),
            ]
                .into_iter()
                .map(|(number, title, text)| {
                    view! {
                        <article class={"panel learn-step"}>
                            <span class={"learn-number"}>{number}</span>
                            <div>
                                <h2>{title}</h2>
                                <p>{text}</p>
                            </div>
                        </article>
                    }
                })
                .collect_view()}
        </div>
        <section class={"panel concept-panel"}>
            <h2>"O que é o quê?"</h2>
            <dl>
                <dt>"Aplicação"</dt>
                <dd>"O projeto: nome, repositório, porta e endereço local."</dd>
                <dt>"Deploy"</dt>
                <dd>
                    "Uma tentativa registrada de colocar uma versão no ar, com histórico e logs."
                </dd>
                <dt>"Imagem"</dt>
                <dd>
                    "O artefato construído. Guardá-lo permite restaurar código sem reconstruir."
                </dd>
                <dt>"Container"</dt>
                <dd>"Uma instância dessa imagem em execução no seu computador."</dd>
                <dt>"Rollback"</dt>
                <dd>
                    "Uma nova publicação usando uma imagem anterior. Não reverte o banco de dados da aplicação."
                </dd>
            </dl>
        </section>
        <div class={"learn-note"}>
            <Icon name={"book"} />
            <p>
                "Este ambiente é local e pessoal. O endereço .localhost funciona no seu computador. Acesso público, autenticação e isolamento multiusuário são uma próxima etapa."
            </p>
        </div>
    }
}

fn storage(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(key)
        .ok()?
}
fn save(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}
