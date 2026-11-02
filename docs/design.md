# Direção visual

Direção definida pelo autor durante o desenvolvimento: imagem de montanhas frias, aparência nórdica, movimento sutil e apresentação profissional. A imagem foi aprovada na conversa antes da composição final do painel.

## Sistema visual

- Grafite azulado e superfícies escuras translúcidas, com contraste para conteúdo operacional.
- Acento verde sálvia para ações e sinais de sucesso; cores de erro/espera discretas.
- Tipografia de sistema, hierarquia curta e mono apenas para identificadores técnicos.
- Movimento lento somente na fotografia; não há gradiente animado.
- Preferência de movimento no painel e respeito a prefers-reduced-motion.
- Confirmações nativas com foco/teclado para ações que afetam disponibilidade.
- Dados reais, estados vazios úteis e aviso quando o executor está desconectado.

Os tokens ficam no começo de `crates/app/assets/style.css`. Os componentes e textos são Rust em `crates/ui/src/app.rs`. Preferências de nome de workspace e movimento ficam apenas no navegador.

## Asset principal

Arquivo do projeto: `crates/app/assets/patagonia.png`.
Origem: ferramenta integrada ImageGen, modo de geração, sem imagem de entrada.
É uma paisagem gerada com IA, não uma fotografia documental de um local específico.
O arquivo original foi preservado; o asset aprovado foi copiado para o projeto.

Prompt usado:

> Use case: photorealistic-natural. Asset type: wide cinematic background photograph for a professional software deployment dashboard. Primary request: cold Chilean Patagonian mountains, Nordic restraint, quiet blue hour. Wide 16:9 composition, rugged snowy Torres del Paine inspired peaks in the lower and middle right, a still dark glacial lake and sparse mist, soft overcast slate-blue sky with substantial calm negative space. Authentic fine photographic rock and snow texture, muted steel blue, deep charcoal, silver grey, a trace of desaturated sage. Underexposed enough to sit behind readable interface panels, sophisticated natural light, minimal visual clutter, no vivid sunset, no northern lights, no buildings, no people, no typography, no UI, no watermark, no artificial gradients. This is a background photograph, not a dashboard mockup.

A interface foi construída em código, não derivada de um mockup de imagem. Os screenshots em `docs/screenshots/` mostram a aplicação em execução.
