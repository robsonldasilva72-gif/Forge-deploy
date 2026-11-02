# Contrato da API

Base padrão: `http://127.0.0.1:3000`. JSON limitado a 16 KiB. Identificadores são UUIDs. Sem autenticação neste ambiente local.

| Método | Rota | Sucesso | Comportamento |
| --- | --- | --- | --- |
| GET | `/` | 200 | Carregador do painel Leptos |
| GET | `/health/live` | 200 | Processo HTTP ativo |
| GET | `/health/ready` | 200 | PostgreSQL responde; 503 se indisponível |
| GET | `/api/apps` | 200 | Até 200 aplicações não removidas |
| POST | `/api/apps` | 201 | Cadastro de aplicação |
| GET | `/api/apps/{id}/deployments` | 200 | Até 200 pedidos da aplicação |
| POST | `/api/apps/{id}/deployments` | 202 | Novo deploy na fila |
| POST | `/api/apps/{id}/rollback` | 202 | Restaurar imagem de publicação anterior |
| POST | `/api/apps/{id}/actions/stop` | 202 | Desativar rota e containers |
| POST | `/api/apps/{id}/actions/cleanup` | 202 | Limpar recursos antigos conforme retenção |
| POST | `/api/apps/{id}/actions/delete` | 202 | Remover recursos e excluir aplicação logicamente |
| GET | `/api/deployments/{id}` | 200 | Resultado e metadados do pedido |
| GET | `/api/deployments/{id}/logs` | 200 | Logs de build/deploy e runtime |
| GET | `/api/snapshot` | 200 | Estado usado pelo painel |
| GET | `/api/events` | 200 SSE | Snapshots do ambiente |
| GET | `/api/deployments/{id}/events` | 200 SSE | Snapshots de logs do pedido |

## Cadastro

Campos: name, repository_url, container_port. Campos desconhecidos são rejeitados. Nome: 1–63 caracteres, primeira letra minúscula, restante com letras minúsculas, números ou hífens, final alfanumérico. URL HTTPS com caminho, sem credenciais, query ou fragmento. Porta: 1–65535.

O cadastro não testa a existência do repositório ou Dockerfile. Esses erros aparecem durante a execução do pedido.

## Pedidos

Deploy/ações administrativas não precisam de corpo. Rollback exige:

```json
{"deployment_id":"UUID_DA_VERSAO_A_RESTAURAR"}
```

A origem deve ser uma publicação concluída da mesma aplicação, com imagem marcada como disponível. A existência física da imagem é confirmada pelo executor.

202 significa aceito, não concluído. Cada POST cria outro pedido; não há deduplicação por chave idempotente. O worker processa sequencialmente.

Deploy/rollback passam por queued, fetching, building, starting, checking_health e ready, ou failed. As operações administrativas usam queued, fetching e o resultado terminal. Building em rollback representa preparação da imagem, sem reconstrução.

Metadados incluem operation, source_deployment_id, snapshot do repositório/porta, commit_sha, image_tag, container_id, host_port, image_available, horários e erro. Alguns campos permanecem null até sua etapa. Portas antigas são metadados históricos; use a rota da aplicação ativa para acesso estável.

## Snapshots e logs

`/api/snapshot` retorna applications, deployments (últimos 100), worker_online e proxy_port. O histórico recente inclui operações de aplicações removidas, preservando seu nome. Listagens têm limites explícitos e ainda não têm paginação.

SSE envia eventos message com JSON e keepalive. Uma mensagem com unavailable=true indica falha de persistência e encerra o stream para reconexão. Não há cursor de eventos: cada mensagem é um snapshot atual, suficiente para reconectar.

Logs retornam logs (cauda do build/deploy, até 64 KiB) e runtime_logs (saída recente do container, limitada separadamente). A saída de runtime é consultada pelo worker; ela pode ficar parada se o executor estiver offline ou a versão não estiver ativa.

## Erros

400: caminho/JSON malformado; 404: recurso/origem não disponível; 409: nome duplicado; 413: corpo excedido; 415: tipo de corpo incorreto; 422: validação; 503: banco indisponível; 500: falha interna.

Erros de domínio/persistência usam error.code e error.message em JSON. Extrações de corpo/caminho usam as respostas padrão do Axum. Não devolvemos erros brutos do PostgreSQL.
