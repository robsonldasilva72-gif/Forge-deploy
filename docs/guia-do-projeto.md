# Entender, modificar e apresentar

O conceito é simples: transformar um repositório em uma aplicação acessível, deixando o caminho visível e guardando uma forma de voltar à versão anterior.

## Sua direção neste projeto

Você escolheu construir uma plataforma para seu portfólio, usar Rust, começar localmente e adotar uma paisagem de montanhas com aparência nórdica e movimento discreto. A implementação foi desenvolvida com assistência de IA. O nome Forge Deploy continua sendo uma proposta inicial; direção, código e evolução estão no seu workspace.

Para apresentar com autoria e segurança, explique as decisões que você adotou e os limites que conhece. Este guia serve para investigar o código e fazer mudanças próprias, não para decorar uma descrição.

## Siga um clique em Publicar

1. `crates/ui/src/app.rs`: `request` envia um POST; os componentes renderizam dados e estados recebidos.
2. `crates/app/src/lib.rs`: `enqueue` valida o identificador e pede a persistência do job.
3. `crates/db/src/lib.rs`: `enqueue` guarda um snapshot do repositório e porta; `claim` toma um pedido atomicamente.
4. `crates/worker/src/main.rs`: o loop mantém exclusividade com uma conexão PostgreSQL dedicada.
5. `crates/worker/src/lib.rs`: `execute` prepara código/imagem, inicia container, verifica HTTP e publica.
6. `crates/docker/src/lib.rs`: Bollard conversa com Docker; portas são publicadas apenas em loopback.
7. `crates/worker/src/routing.rs`: Caddy muda o destino; a versão ativa é registrada; resultados incertos são reconciliados.
8. `crates/app/src/live.rs`: SSE leva os estados/logs ao navegador, sem acesso direto do painel ao Docker.

O Graft permite explorar esses caminhos com `trail ask`, `trail callers` e `trail skeleton`, antes de abrir arquivos inteiros.

## Separe os conceitos

- **Aplicação:** identidade/configuração duradoura de um projeto.
- **Pedido:** deploy ou operação administrativa que entra na fila.
- **Deploy:** uma tentativa de executar uma versão, com resultado, logs e horários.
- **Imagem:** artefato construído, identificado e guardado no Docker.
- **Container:** instância descartável da imagem.
- **Versão ativa:** o deploy que recebe o tráfego do endereço estável.

Um deploy antigo pode continuar concluído mesmo depois de seu container ser removido. Isso descreve o resultado histórico, não que ele continua rodando. O estado da aplicação é monitorado separadamente.

## Por que o rollback não é só alterar um ponteiro?

O container antigo pode já ter sido removido. O executor cria uma instância nova da imagem guardada, verifica HTTP e então muda a rota. Isso permite recuperar código sem refazer o build. O banco de dados da aplicação não faz parte dessa reversão.

## Por que persistir uma fila?

O servidor HTTP não deve ficar preso a um build longo. O pedido continua no banco se o processo reiniciar. A execução é serializada por um lock de sessão; ao reiniciar, pedidos interrompidos são reconciliados. Isso não torna todos os efeitos externos transacionais nem garante exatamente uma execução.

## Mudanças próprias com impacto compreensível

1. Troque o nome/identidade visual em `crates/ui/src/app.rs`, no HTML e no CSS. Mantenha os identificadores internos estáveis inicialmente.
2. Ajuste os tokens no começo de `crates/app/assets/style.css` e compare com os screenshots.
3. Acrescente um campo de caminho de health check: acompanhe formulário, domínio, migration, snapshot do job e executor; escreva um teste de uma aplicação sem HTTP 2xx na raiz.
4. Adicione uma aplicação sua e conte uma falha real que encontrou e resolveu.
5. Decida uma política própria de retenção de versões e teste que ela preserva a imagem ativa.

## Perguntas que você deve conseguir responder

- O que acontece se a nova versão não responder?
- Por que uma imagem pode existir sem um container ativo?
- O que acontece se o worker cair no meio da publicação?
- Como a plataforma evita remover containers de outros projetos?
- Por que um painel local sem autenticação não pode simplesmente ser exposto publicamente?
- Qual problema você resolveria primeiro para usar múltiplas máquinas?

As respostas estão nos testes, nas condições do executor e nas decisões de arquitetura. Demonstre o sistema funcionando e abra o código para sustentar o que você explica.

## Formatação da interface

O código dos componentes está formatado com leptosfmt. Há uma configuração no arquivo `leptosfmt.toml` que preserva chaves em valores de atributos; isso evita ambiguidade em expressões que contêm o operador maior que. A ferramenta é opcional para executar a aplicação.

```bash
cargo install leptosfmt --version 0.1.33 --locked --root .tools
.tools/bin/leptosfmt -c leptosfmt.toml crates/ui/src/app.rs
cargo fmt --all
```
