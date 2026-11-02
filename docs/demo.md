# Demonstração para o portfólio

## Preparação

Execute `python3 scripts/dev.py start --build`, confirme executor conectado e use uma aplicação pública sua com Dockerfile. A demonstração inicial usa `docker/welcome-to-docker` como aplicação hospedada, não como código da plataforma.

## Roteiro de 90 segundos

1. Abra a visão geral: explique que painel, servidor e executor são Rust; os números são dados reais.
2. Abra Como funciona: aplicação, imagem, container e versão ativa são conceitos diferentes.
3. Cadastre seu repositório, informe a porta e publique.
4. Mostre as etapas e a saída ao vivo. Abra o endereço .localhost.
5. Publique outra versão. Mostre que o destino só muda depois do teste HTTP.
6. Restaure uma imagem anterior. Mostre a mensagem de reutilização, sem rebuild.
7. Mostre a confirmação de limpeza e explique a retenção; não remova sua demonstração principal apenas para gravar.
8. Termine mostrando um teste de falha e um trecho de código que você consegue explicar.

## O que sustenta a apresentação

- Testes reais com PostgreSQL, Docker e Caddy.
- Teste com requisições durante uma troca de versão.
- Falha de imagem ausente preservando a versão ativa.
- Resultado incerto do proxy preservando o candidato até a reconciliação.
- Encerramento de processos por timeout e limpeza limitada a recursos identificados.

## Limites que vale explicar

O ambiente é local e usa código confiável. Não há autenticação pública ou isolamento multiusuário. O teste de troca cobre requisições HTTP curtas; não é garantia universal de zero downtime para conexões longas. Rollback não reverte banco de dados.

Adicione ao README a URL do seu repositório e, depois de gravar, o link do vídeo. A publicação no seu GitHub não foi feita automaticamente.
