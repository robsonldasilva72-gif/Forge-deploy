# Executor

O worker é um processo Rust separado com Tokio, SQLx, Bollard e cliente Caddy. O painel nunca recebe acesso ao Docker.

## Execução normal

Na raiz, `python3 scripts/dev.py start --build` inicia os serviços e processos. Use status/stop no mesmo script para gerenciar apenas essa execução. Os logs ficam em .forge.

Também é possível executar manualmente, em terminais separados:

```bash
docker compose -f infra/compose.yaml up -d --wait
export DATABASE_URL='postgres://forge:forge_local@127.0.0.1:5432/forge'
cargo run -p forge-deploy-app
```

```bash
export DATABASE_URL='postgres://forge:forge_local@127.0.0.1:5432/forge'
cargo run -p forge-deploy-worker
```

O usuário deve ter acesso ao socket Docker. A sessão pode precisar de login novo ou sg docker após entrar no grupo. O gerenciador Python usa sg quando necessário. Não execute toda a plataforma como root para contornar isso.

`--once` processa no máximo um pedido e encerra. Uma falha da aplicação é persistida como failed e não impede o próximo pedido. Falhas que impedem determinar com segurança a publicação encerram o executor e preservam o candidato para reconciliação.

## Configuração

| Variável | Padrão | Uso |
| --- | --- | --- |
| DATABASE_URL | obrigatório no binário; padrão local no gerenciador | PostgreSQL da API e worker |
| FORGE_PORT | 3000 | Porta da API/painel |
| FORGE_DEPLOY_TIMEOUT_SECS | 600 | Timeout de execução de um pedido |
| FORGE_DOCKER_SOCKET | /var/run/docker.sock | Socket Unix de Bollard e CLI |
| FORGE_CADDY_URL | http://127.0.0.1:2019 | Administrador Caddy, somente HTTP local |
| FORGE_PROXY_PORT | 8080 | Porta HTTP local dos endereços estáveis |
| FORGE_ASSET_DIR | assets da crate app | Diretório do painel compilado |
| RUST_LOG | info | Logs dos processos |

O binário lê o ambiente do processo. O gerenciador também lê .env sem expansão de shell. FORGE_PROXY_PORT deve ser igual na API e worker. A configuração explícita de socket é usada no build; DOCKER_HOST e DOCKER_CONTEXT não selecionam outro daemon para o build.

## Contrato da aplicação hospedada

Repositório público HTTPS confiável, Dockerfile na raiz, branch padrão. Aplicação HTTP em 0.0.0.0 na porta cadastrada, com resposta 2xx em `/` em até 30 segundos. Sem submódulos, credenciais Git, variáveis de ambiente da aplicação ou migrations automáticas nesta etapa.

O worker registra o SHA, constrói usando docker build e respeita .dockerignore. No rollback, reutiliza a imagem existente e cria uma nova instância.

## Limites de recursos

Container: 256 MiB de memória, uma CPU, 128 processos, capabilities removidas, no-new-privileges e rotação de logs. Builds ainda usam recursos/cache/rede do daemon e só devem executar código confiável. Isso não é sandbox de código hostil.

Portas são publicadas apenas em loopback. Imagens e containers são identificados por nomes e labels de propriedade. Nunca há prune global.

Workspaces temporários são removidos no encerramento normal; SIGKILL ou queda do sistema podem deixar diretórios temporários. O banco guarda uma cauda limitada dos logs; não injete segredos na saída da aplicação/build.

## Recuperação e limpeza

Leia [a arquitetura](architecture.md) para a ordem entre proxy, banco e limpeza. A imagem ativa e até duas outras imagens recentes são preservadas na limpeza. Remoção de uma aplicação desativa sua rota e limpa recursos identificados, preservando histórico no banco.

As integrações estão em crates/worker/tests e são opt-in. TEST_DATABASE_URL deve apontar para um banco separado, sem worker de produção. O teste platform_flow cria um servidor Caddy isolado, sem sobrescrever as rotas do workspace.
