# Decisões de engenharia

## Rust no servidor e no navegador

Axum atende HTTP; Leptos CSR compila os componentes para WebAssembly. Um painel operacional local não precisa de SEO, e CSR permite compartilhar uma API HTTP explícita sem acoplar o navegador à persistência. Não usamos SSR nesta implementação. Um pequeno carregador JavaScript inicializa o módulo gerado; a lógica da interface é Rust.

## Uma máquina, um executor

O recorte inicial cabe num computador Linux e permite observar o sistema inteiro. PostgreSQL também guarda a fila; não adicionamos Redis ou um broker antes de haver necessidade. O lock de sessão serializa workers, e a tomada de trabalho é atômica. Não prometemos exactly once.

## SQL explícito

Consultas com bind parameters e FromRow; migrations embutidas no binário. As consultas são verificadas por integrações PostgreSQL reais, não por macros de consulta com validação em compilação.

## CLI para construir; Bollard para executar

O cliente Docker já conhece as regras de .dockerignore e contexto de build. O worker controla sua execução e saída; Bollard gerencia containers pelo mesmo socket. Isso evita reproduzir regras do cliente Docker.

## Uma imagem guardada é o caminho de volta

Rollback reutiliza um artefato existente, inicia um novo container e verifica HTTP. Não depende de um container antigo continuar vivo. O commit e a imagem usados ficam no histórico. Limpeza pode remover a possibilidade de restaurar versões antigas, por isso o painel explica a retenção antes da confirmação.

## Publicação com compensação

Caddy e PostgreSQL não compartilham uma transação. O worker trata perda de confirmação como incerteza, preserva recursos e reconcilia a partir do banco. Atualizações periódicas do proxy são serializadas com publicação para não sobrescrever mudanças em andamento.

## SSE com snapshots

Reconexão é simples: o navegador recebe o estado atual. Não é necessário reconstruir uma sequência de mensagens perdidas. Logs são limitados para não crescer sem controle, e a interface separa build/deploy da saída recente da aplicação.

## Design legível sobre uma imagem

A direção nórdica e a paisagem foram escolhidas pelo autor. A imagem tem movimento lento e discreto; superfícies escuras mantêm a hierarquia e a legibilidade. Preferências de movimento e identidade do workspace são locais ao navegador.

## Limite local deliberado

A API não possui autenticação e o Docker concede controle privilegiado. Este MVP não é um serviço público multiusuário. Antes de hospedá-lo para terceiros, seriam necessários autenticação, autorização, isolamento de builds, política de segredos, quotas e outra estratégia operacional.
