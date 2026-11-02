# Forge Deploy

**Status: MVP local concluído.**

**Do código ao ar.** Uma plataforma local de deploy, escrita em Rust, para publicar aplicações com Dockerfile, acompanhar o processo e restaurar versões anteriores.

![Visão geral do Forge Deploy](docs/screenshots/overview.png)

## O que funciona

- **Painel Leptos:** aplicações, atividade, histórico de versões e explicação do fluxo.
- **Logs ao vivo:** saída do build/deploy e saída recente da aplicação via SSE, com reconexão automática.
- **Gerenciamento:** publicar, interromper, restaurar, limpar imagens antigas e remover aplicações.
- **Endereço estável:** Caddy encaminha `nome.localhost:8080` para a versão ativa.
- **Troca de versões:** o candidato passa pela verificação HTTP antes de receber tráfego.
- **Rollback sem rebuild:** uma nova instância usa a imagem de uma publicação anterior.

O fundo é uma imagem inspirada na Patagônia, com movimento discreto, preferência para desativá-lo e respeito a movimento reduzido. Os dados do painel são reais; não há métricas simuladas.

## Executar

Pré-requisitos: Linux, Rust estável via rustup, Python 3, Git e Docker Engine com Compose/Buildx. O usuário precisa de acesso ao socket Docker. [Instalação neste Mint](docs/setup-docker.md).

Na raiz do projeto:

```bash
python3 scripts/dev.py start --build
```

A primeira execução compila a interface para WebAssembly e instala a ferramenta `wasm-bindgen` localmente em `.tools/`. Não é necessário Node.js para construir o painel.

Abra **[http://127.0.0.1:3000](http://127.0.0.1:3000)**.

```bash
python3 scripts/dev.py status
python3 scripts/dev.py stop
python3 scripts/dev.py start
```

`--build` recompila depois de mudanças de código. Sem ele, os binários/assets existentes são reutilizados. O comando stop encerra apenas os processos registrados pelo script; preserva aplicações publicadas, Caddy e PostgreSQL. Logs dos processos ficam em `.forge/`.

O gerenciador lê `.env`, se existir, sem expansão de shell. As variáveis já exportadas têm prioridade. Os binários Rust leem apenas o ambiente do processo. [Referência de configuração](.env.example).

## Seu primeiro deploy

1. Cadastre um repositório público seu ou explicitamente confiável, com Dockerfile na raiz.
2. Informe a porta em que a aplicação escuta **dentro do container**, em `0.0.0.0`.
3. Clique em **Publicar** e acompanhe os logs.
4. Quando a publicação concluir, abra `http://nome-da-aplicacao.localhost:8080`.
5. Na página da aplicação, use **Restaurar** para publicar novamente uma imagem anterior.

O health check exige HTTP 2xx em `/` em até 30 segundos. Um pedido aceito entra na fila; só a conclusão confirma a publicação. Sem executor ativo, os pedidos aguardam e o painel informa isso.

## Como funciona

O painel pede uma operação à API Axum. A API persiste o pedido no PostgreSQL. Um worker separado assume o trabalho, constrói a imagem e cria o container. Após o teste HTTP, atualiza a rota Caddy e registra a versão ativa. O navegador recebe atualizações por SSE.

Uma **aplicação** é a definição de um projeto. Um **deploy** é uma tentativa versionada. Uma **imagem** é o artefato que pode ser reutilizado. Um **container** é uma instância em execução. **Rollback restaura código, não o banco de dados da aplicação.**

Veja [o guia do projeto](docs/guia-do-projeto.md) para acompanhar o caminho de um clique até o container e entender as decisões.

## Stack e organização

| Diretório | Responsabilidade |
| --- | --- |
| `crates/ui` | Painel Leptos CSR, compilado em Rust para WebAssembly |
| `crates/app` | API Axum, eventos SSE e assets do painel |
| `crates/core` | Validação e estados de domínio |
| `crates/db` | SQLx, migrations, histórico e fila PostgreSQL |
| `crates/worker` | Execução de jobs, recuperação, publicação e Caddy |
| `crates/docker` | Ciclo de vida dos containers via Bollard |
| `infra` | PostgreSQL e Caddy locais via Compose |
| `scripts` | Build da interface e execução dos processos |

O build das aplicações usa a CLI Docker para respeitar `.dockerignore`. O runtime usa Bollard. A interface não usa React ou TypeScript; o pequeno carregador JavaScript e o glue gerado inicializam WebAssembly.

## Verificação

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p forge-deploy-ui --target wasm32-unknown-unknown -- -D warnings
```

As integrações que dependem de serviços são explicitamente ignoradas no comando padrão. Use um PostgreSQL de teste separado:

```bash
export TEST_DATABASE_URL='postgres://forge:forge_local@127.0.0.1:5432/forge_test'
cargo test -p forge-deploy-db --test persistence -- --ignored
cargo test -p forge-deploy-worker --test control -- --ignored
cargo test -p forge-deploy-worker --test docker_flow -- --ignored --nocapture
cargo test -p forge-deploy-worker --test platform_flow -- --ignored --nocapture
```

Crie previamente o banco `forge_test`. Os testes criam schemas isolados; o teste Caddy usa um servidor isolado e remove apenas seus próprios recursos. Docker e Caddy devem estar acessíveis. [Resultados e limites da verificação](docs/verification.md).

## Limites conscientes

Este é um ambiente **local, pessoal e para código confiável**. Não há autenticação, isolamento para builds hostis, suporte multiusuário ou HTTPS público. Acesso ao Docker é privilegiado. Publicar esta API na internet exige outro modelo de acesso e isolamento.

O worker é único. Estado de processo e logs de runtime são consultados aproximadamente a cada três segundos; o health check HTTP é de publicação, não monitoramento contínuo. A troca de rota e o banco não compartilham uma transação distribuída: há compensação e reconciliação quando o resultado é incerto. Não há promessa de execução exatamente uma vez ou de preservação de conexões longas.

A limpeza preserva a imagem ativa e até duas outras imagens recentes. Não executamos prune global nem apagamos arquivos do repositório original. Imagens externas ou compartilhadas com containers fora da plataforma podem exigir intervenção manual.

## Documentação

- [Guia para entender, modificar e apresentar](docs/guia-do-projeto.md)
- [Arquitetura e recuperação](docs/architecture.md)
- [Contrato da API](docs/api.md)
- [Executor e configuração](docs/executor.md)
- [Decisões de engenharia](docs/decisions.md)
- [Direção visual e origem dos assets](docs/design.md)
- [Roteiro de demonstração](docs/demo.md)

A CI nativa compila/verifica Rust e a interface. Uma segunda workflow permite executar as integrações Docker/Caddy manualmente no GitHub. As workflows estão preparadas; sua execução no GitHub depende de publicar o repositório na sua conta.
