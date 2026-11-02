# Verificação da implementação

## Ambiente utilizado

Linux Mint, Rust 1.99.1, Leptos 0.8.23, wasm-bindgen 0.2.130, Docker Engine 29.9.2 e PostgreSQL 17/Caddy via Compose.

## Verificações aprovadas

| Verificação | Evidência |
| --- | --- |
| Rust nativo | Compilação do workspace, rustfmt e Clippy com warnings como erro |
| Interface Rust/WASM | Build release, geração do módulo web e Clippy para wasm32 |
| Domínio e HTTP | 9 testes independentes de banco |
| PostgreSQL | Teste persistence: migrations, validações, snapshots, claims concorrentes, transições e limites UTF-8 dos logs |
| Controle do worker | Teste control: exclusão entre workers, liberação do lock, stdout/stderr e cancelamento de processos |
| Deploy Docker | Teste docker_flow: Git real, imagem, HTTP, falhas, timeout e recuperação |
| Plataforma/Caddy | Teste platform_flow: versões diferentes, tráfego durante troca, rollback sem rebuild, parada, retenção e remoção |
| Proxy e incerteza | Reconciliação concorrente sem perder a publicação, restauração de configuração resetada e preservação do candidato quando a confirmação do proxy é incerta |

Total: **13 testes aprovados**, contando as quatro integrações executadas explicitamente. O comando cargo test --workspace sozinho ignora as integrações que exigem serviços.

## Fluxos verificados no navegador

- Visão geral alimentada por dados reais, com executor conectado.
- Publicação da demonstração por um clique no painel, acompanhando estados e logs sem atualizar a página.
- Rollback por confirmação no painel, concluído com reutilização da imagem.
- Navegação de aplicações, histórico, explicação de conceitos e preferências.
- Formulário com campos rotulados, mensagens de erro e fechamento por Escape.
- Controle de movimento alterando efetivamente animation-name, com preferência restaurada ao final.
- Revisão em tela pequena: conteúdo sem overflow horizontal; navegação com rótulos acessíveis.
- Screenshots reais em docs/screenshots, incluindo visão geral e versão móvel.

## Operação local

O gerenciador start/status/stop foi testado. O encerramento com um stream SSE aberto foi verificado: o servidor fecha os streams e permite reinício sem manter a porta ocupada. Aplicações, Caddy e banco permanecem preservados ao parar API/worker.

A saída recente do container ativo foi consultada pela API no campo runtime_logs. O endereço estável forge-demo.localhost:8080 respondeu HTTP 200 através do Caddy.

## Limites da evidência

Os testes de tráfego usam requisições HTTP curtas e não provam preservação de todos os protocolos ou conexões longas. A revisão de acessibilidade foi funcional/visual, não uma certificação formal. O ambiente é local e confiável, sem teste de isolamento multiusuário ou exposição pública.

As workflows estão configuradas, mas não foram executadas no GitHub porque o repositório remoto do autor ainda não foi configurado. O roteiro de demonstração está pronto; um vídeo externo não foi gravado ou publicado automaticamente.

O teste platform_flow remove suas próprias imagens e containers; docker_flow remove seus containers e pode preservar imagens/cache de build para inspeção. Não há prune global.
