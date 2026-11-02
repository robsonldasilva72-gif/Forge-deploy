# MVP local concluído — escopo e evolução

## MVP local implementado

- [x] Workspace Rust e domínio validado.
- [x] API Axum, PostgreSQL, migrations, fila e histórico.
- [x] Worker com lock, timeout, recuperação e diagnóstico.
- [x] Deploy real com Git, Docker e teste HTTP.
- [x] Painel Leptos em WebAssembly.
- [x] Logs de build/deploy ao vivo e saída recente da aplicação.
- [x] Gerenciamento: interromper, remover e limpar recursos antigos.
- [x] Endereços locais estáveis com Caddy.
- [x] Troca de versões verificadas e rollback sem rebuild.
- [x] Testes reais de troca, retenção e recuperação de incerteza.
- [x] Inicialização local, documentação conceitual, roteiro e workflows de CI.

## Próximas evoluções, fora deste MVP

- Publicar o repositório na conta do autor e executar as workflows no GitHub.
- Gravar uma demonstração com um projeto próprio.
- Branch/commit selecionável e health check configurável.
- Paginação e busca de histórico.
- Autenticação/autorização antes de qualquer exposição pública.
- Isolamento de builds, segredos e quotas para múltiplos usuários.
- Múltiplas máquinas e coordenação com garantias mais fortes.
