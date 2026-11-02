# Ambiente local Linux

O Compose inicia PostgreSQL e Caddy. A API/painel e o worker são processos do host, iniciados pelo gerenciador Python ou Cargo.

- PostgreSQL: 127.0.0.1:5432, com volume persistente.
- Caddy: rede host para acessar os containers publicados em loopback; HTTP em 127.0.0.1:8080 e administrador em 127.0.0.1:2019.
- Os volumes Caddy preservam seus dados; a configuração inicial pertence ao projeto. O worker reconcilia as rotas a partir do PostgreSQL.

As credenciais padrão são exclusivas de desenvolvimento. docker compose down preserva volumes, mas interrompe banco e proxy. Não use -v se quiser preservar os dados.

O modo network_mode: host desta configuração tem como alvo Linux.
