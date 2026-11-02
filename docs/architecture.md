# Arquitetura e recuperação

## Fronteiras

O painel Leptos é compilado para WebAssembly e servido como asset pela API Axum. Ele só acessa HTTP/SSE da própria API. Não recebe credenciais PostgreSQL ou acesso ao socket Docker.

A API valida entradas, persiste pedidos e fornece snapshots. O worker é outro processo: ele controla Git, CLI Docker, Bollard e a configuração Caddy. O PostgreSQL guarda aplicações, histórico, fila, logs e a versão ativa.

Caddy usa rede host no Linux para alcançar containers publicados somente em loopback. Tanto sua porta HTTP 8080 quanto o administrador 2019 são vinculados a 127.0.0.1. PostgreSQL também é publicado apenas localmente.

## Do pedido à publicação

1. A API guarda um snapshot do repositório e porta no pedido queued.
2. O worker toma um pedido com UPDATE atômico e SKIP LOCKED, marcando fetching.
3. Em deploy, clona a branch padrão, registra o commit e constrói a imagem. Em rollback, valida a imagem histórica e a reutiliza.
4. Inicia um container candidato numa porta local dinâmica.
5. Exige HTTP 2xx em `/`; a versão anterior continua sendo o destino do endereço estável.
6. Atualiza a rota Caddy do projeto e registra, em uma transação PostgreSQL, o resultado ready e a nova versão ativa.
7. Retira o container anterior. A imagem permanece para rollback e retenção.

Não há migração automática do banco da aplicação, entrada de segredos de build, escolha de branch ou repositórios privados neste MVP.

## Banco e proxy não são uma transação distribuída

O Caddy aplica cada alteração de configuração de forma atômica, mas não participa da transação PostgreSQL. O worker compensa falhas restaurando a configuração a partir do banco. Se não puder determinar/restabelecer o estado, preserva o candidato e encerra para reconciliação, em vez de removê-lo sem saber se já recebe tráfego.

Ao iniciar, o worker reconstrói as rotas a partir das versões ativas no banco antes de remover containers incompletos. Jobs interrompidos tornam-se failed e podem ser reenviados. Um monitor reconcilia Caddy periodicamente e só altera a configuração quando ela diverge. Uma exclusão assíncrona serializa essa reconciliação com publicação e interrupção, evitando que o monitor reponha uma rota antiga entre a mudança do proxy e o commit do banco.

Isso reduz janelas de inconsistência; não equivale a exatamente uma execução ou tolerância geral a partições distribuídas. Conexões longas podem depender do encerramento gracioso implementado pela aplicação.

## Exclusividade e processos

Um advisory lock de sessão em conexão dedicada impede dois workers ativos no mesmo banco. Essa conexão nunca retorna ao pool. Um heartbeat persistido e monitorado indica presença do executor. Falha da conexão encerra o processamento.

Git e Docker CLI são iniciados sem shell montado a partir de dados do usuário, em grupos de processo próprios. Cancelamento e timeout encerram esses processos e seus auxiliares.

## Logs e estado atual

SSE entrega snapshots, não um log de eventos com garantia de entrega exatamente uma vez. Na reconexão, o navegador recebe novamente o estado atual.

- Snapshot de aplicações/pedidos: aproximadamente a cada segundo, somente quando muda.
- Snapshot de logs do pedido selecionado: aproximadamente a cada meio segundo, somente quando muda.
- Logs de build/deploy: cauda limitada a 64 KiB, com marcador de truncamento.
- Logs de runtime: saída recente do container ativo, consultada aproximadamente a cada três segundos e limitada separadamente.
- Estado de runtime: running, stopped ou unknown, com horário da última consulta.

Ready é um resultado histórico de publicação; não significa que um container antigo continua rodando. A aplicação aponta para sua versão ativa e tem estado de runtime separado.

## Gerenciamento e retenção

Interromper, remover e limpar também são pedidos persistidos. O navegador nunca faz operações Docker diretamente.

Interromper remove a rota e os containers, mantendo imagens. Restaurar uma versão cria uma nova instância. Limpar preserva a imagem ativa e até duas outras imagens recentes. Remover desativa a aplicação, remove recursos identificados e preserva seu histórico no banco como exclusão lógica.

Nomes e labels de propriedade são verificados antes da remoção. Não há prune global. Recursos compartilhados fora da plataforma podem impedir remoção; o erro fica no histórico para intervenção, sem forçar a exclusão.

## Falhas operacionais

- API desligada: aplicações já publicadas continuam servidas pelo Caddy.
- Worker desligado: pedidos aguardam; o painel identifica ausência pelo heartbeat.
- Proxy reiniciado: o worker reconcilia a configuração a partir do banco.
- Resultado de publicação incerto: candidato preservado e worker encerrado; reinício reconcilia antes da limpeza.
- Operação administrativa parcialmente concluída: o histórico registra falha; o estado real pode já estar interrompido. Repetir a operação permite concluir a limpeza.
