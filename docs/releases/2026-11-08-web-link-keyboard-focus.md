# Item 18: indicação visual de foco nos links Web

Data: 2026-11-08. Estado: implementação parcial; checagem estática do item 18 aprovada; revisão textual final concluída sem bloqueadores. Itens 1–18 permanecem ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA a pedido da usuária.

## Recorte e contrato

Em apps/browser-desktop/src/shell.rs, o laço de links conserva a Response retornada pela interação e usa o helper page_link_focus_rect para obter a interseção visível. Pinta um contorno quando has_focus() é verdadeiro e essa interseção tem área positiva. O indicador independe da seleção de elemento pelo assistente de IA: seleção e foco não são o mesmo estado.

- Preserva ID, retângulo de interação, Sense::click, cursor, tooltip, destino e fluxo clicked() existentes.
- Usa o foco e a travessia nativos do egui, sem outro handler de Tab, Enter ou Space e sem duplicar a ativação.
- O contorno é interno ao retângulo do link e respeita o clipping do viewport. Não amplia a área clicável nem pinta fora da região permitida.
- Não fabrica nome acessível a partir de href e não acrescenta semântica de link a uma árvore de acessibilidade.
- Não muda IPC, motor, broker, política de navegação, permissões ou fronteiras de confiança. Não exige novo ADR.

## Limites

Este recorte torna visível o foco nativo que o widget já pode receber; não implementa navegação por teclado Web completa. Os controles são registrados antes dos links e links fora do viewport são omitidos desse caminho: não se promete ordem Tab completa do DOM nem rolagem automática até todos os links. Links fragmentados continuam sem identidade DOM unificada, podendo corresponder a várias regiões nativas.

Permanecem pendentes árvore de acessibilidade do conteúdo, integração semântica com leitores de tela, tabindex, foco DOM, eventos focus/blur e critérios completos de acessibilidade. Não há comprovação de contraste, comportamento visual, ativação por teclado ou compatibilidade com tecnologia assistiva por execução.

## Evidência

Checagem estática do item 18 aprovada; revisão textual final concluída sem bloqueadores. A checagem do item 17 (sessão 89428, exit 0, 1,44 s, somente warning preexistente Stmt::Function) pertence ao incremento anterior e não valida este recorte.

Foram escritas três regressões puras de geometria/foco/seleção de IA, não executadas. Elas não são testes de renderização ou de navegação nativa. Nenhum teste, fixture, aplicativo, egui, GPU ou navegação foi executado nesta etapa documental; nenhum fmt/check/build foi executado por esta etapa. Código e revisão textual não substituem validação funcional. Não houve reconstrução de dist/Noir, commit, push ou publicação.

Veja o [índice dos incrementos da rodada](2026-11-08-roadmap-first-pass.md), a [matriz de capacidades](../architecture/capabilities.md) e o [roadmap detalhado](../roadmap/implementações%20Planejadas.md).

## Checagem estática registrada

Na integração central do item 18, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. A sessão 8690 terminou com exit 0; o check durou 0,79 s. Permaneceu apenas o warning preexistente Stmt::Function em parser.rs:179. A revisão textual final terminou sem bloqueadores. Nenhum teste ou aplicativo foi executado. Aprovação estática não comprova comportamento funcional, visual ou acessibilidade; item 18 permanece ABERTO/parcial.
