# Docker no Linux Mint

Ambiente verificado: Linux Mint 22.3 com base Ubuntu 24.04 (noble).
O Docker documenta a instalação para Ubuntu; distribuições derivadas, como Mint,
não são oficialmente suportadas, embora o procedimento possa funcionar.

Fonte: [instalação oficial do Docker Engine](https://docs.docker.com/engine/install/ubuntu/).

## Instalação interativa

A instalação exige privilégios administrativos. O agente não tem sudo sem senha
neste ambiente. Execute no seu próprio terminal, na raiz do projeto:

```sh
bash scripts/install-docker-mint.sh
```

O script instala Engine, CLI, Buildx e Compose a partir do repositório oficial,
inicia o serviço e testa hello-world. A senha é digitada somente no terminal.
Ele não remove pacotes conflitantes e para se já houver configuração do
repositório. Revise a documentação oficial nesses casos. O procedimento ainda
não foi executado neste computador; uma falha parcial pode exigir revisar o
repositório antes de repetir.

## Permissões

Inicialmente, execute os comandos Docker com sudo:

```sh
sudo docker compose -f infra/compose.yaml up -d --wait
sudo docker info
```

Para usar Docker sem sudo, o guia oficial explica a configuração do grupo docker:
[passos após instalação](https://docs.docker.com/engine/install/linux-postinstall/).
Esse grupo concede privilégios equivalentes a root; a alteração não foi feita
pelo script. O futuro worker precisará de acesso ao Docker, mas não deve exigir
executar toda a aplicação Rust como root.

O PostgreSQL no Compose está restrito a 127.0.0.1.
