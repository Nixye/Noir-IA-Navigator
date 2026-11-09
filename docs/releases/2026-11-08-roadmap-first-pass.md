# Índice de incrementos do roadmap: rodada de 2026-11-08

Nota histórica: esta primeira passagem preserva as instruções e evidências disponíveis na época, inclusive testes então adiados. A usuária posteriormente autorizou testes e ciclos de correção; o estado atual e os resultados executados estão no [baseline funcional](../validation/2026-11-08-functional-baseline.md). O adiamento abaixo não é uma proibição vigente, e os resultados posteriores não reescrevem as entregas antigas como testadas.


Este documento orienta a revisão do código-fonte desta rodada, não declara um navegador completo. A numeração abaixo é a de [implementações planejadas](../roadmap/implementações%20Planejadas.md), não a dos seis marcos de [evolução](../roadmap/implementation.md). **Todos os itens 1–18 continuam ABERTOS/parciais. Item 19: EXECUÇÃO ADIADA a pedido da usuária.**

## Incrementos e lacunas

| Item | Código acrescentado nesta rodada e referência principal | Lacunas que mantêm o item aberto |
| --- | --- | --- |
| 1. JavaScript próprio | 20 incrementos limitados, de [linguagem, objetos e GC](2026-11-08-javascript-roadmap-item1.md) a [delete Member resumível](2026-11-08-resumable-delete.md). A sequência completa está abaixo. | ECMAScript integral, módulos sob demanda/avaliação assíncrona, async/generators completos, Proxy/Reflect completos e Intl. |
| 2. WebAssembly | [Validação e execução numérica stateless](2026-11-08-stateless-webassembly.md), via wasmi sem JIT e sob quotas. | Imports, memória/tabelas, start, estado mutável persistente e integração WebAssembly completa. |
| 3. HTML/documentos/DOM | [html/head/body normalizados, fragmentos contextuais e getters](2026-11-08-normalized-html-documents.md). | Construção WHATWG completa, namespaces e DOM completo. |
| 4. Ciclo de vida | [readyState, eventos iniciais e defer](2026-11-08-document-lifecycle.md). | Async real, parser incremental, rAF e event loop Web completo. |
| 5. Eventos/entrada | [Event/CustomEvent e dispatchEvent síncrono](2026-11-08-programmatic-events.md). | Ações padrão nativas, edição/entrada completas, eventos especializados e Shadow DOM. |
| 6. CSS/cascata | [Especificidade lexicográfica e IDs compostos](2026-11-08-css-specificity.md). | CSSOM, custom properties e camadas/origens completas. |
| 7. Layout/geometria | [Alinhamento transversal Flex row/row-reverse nowrap](2026-11-08-flex-cross-axis.md), com altura definida. | Stretch geral, wrap/coluna, Grid e geometria Web completos. |
| 8. Texto/fontes | [Whitespace compartilhado entre fluxo e medida por nó](2026-11-08-text-whitespace.md). | Medição intrínseca completa entre irmãos, shaping/bidi, fontes e fallback completos. |
| 9. Rede/recursos | [Parsing limitado de Content-Type/MIME/charset](2026-11-08-http-content-type.md) no broker. | Fetch/CORS completos, sniffing, cache HTTP e streaming. |
| 10. Navegação/contextos | [Rolagem bidimensional por entrada do histórico nativo](2026-11-08-history-scroll.md), em memória. | BFCache, restauração de formulários/VM, History API JS e persistência após reinício. |
| 11. Persistência/workers/offline | [sessionStorage limitado por aba/origem HTTP(S)](2026-11-08-session-storage.md). | localStorage, cookies, IndexedDB, workers e offline. |
| 12. Imagens/gráficos | [Dimensões naturais separadas da textura reduzida](2026-11-08-natural-image-dimensions.md). | Replaced elements completos, srcset/picture, Canvas/WebGL/WebGPU e getters JS naturalWidth/naturalHeight. |
| 13. Áudio/vídeo | [Metadados de WAV PCM local e audio.duration](2026-11-08-wav-metadata.md). | Reprodução, vídeo, mídia remota e streaming. |
| 14. Segurança/dispositivos | [crypto.subtle.digest SHA-256/384/512 limitado](2026-11-08-bounded-crypto-digest.md), somente no contexto HTTPS delimitado. | Secure contexts completos, RNG, chaves, assinaturas, WebRTC/WebAuthn e dispositivos. |
| 15. Segurança/isolamento | [Cotas antecipadas no codec IPC e writer privado limitado](2026-11-08-bounded-ipc-codec.md), wire v7 preservado. | Orçamento global de memória/CPU, novas provas de isolamento e SOP/CSP completos. |
| 16. Driver próprio | [Culling de glifos antes da rasterização/atlas](2026-11-08-clipped-glyph-raster.md), preservando avanço e kerning. | Pipeline gráfico Web completo, cobertura específica pendente e desempenho medido. |
| 17. Automação/agente | [Observação textual limitada na extração e no JSON](2026-11-08-bounded-ai-observation.md), filtrada por permissão. | Observação DOM/layout/acessibilidade/imagem completa, automação geral e avaliação de qualidade/segurança. |
| 18. Acessibilidade/teclado | [Contorno do foco nativo nos links Web](2026-11-08-web-link-keyboard-focus.md), independente da seleção de IA. Checagem estática aprovada; revisão textual final concluída sem bloqueadores. | Ordem Tab DOM completa, identidade unificada de links fragmentados, árvore acessível, leitores de tela, tabindex e eventos de foco DOM. |
| 19. Testes/compatibilidade | **EXECUÇÃO ADIADA** a pedido. Fontes de regressão e fixtures preparadas não são resultados. | Test262, WPT, fuzzing, testes funcionais, navegação em sites e comprovação de compatibilidade não executados nesta rodada. |

## Os 20 incrementos do item 1

1. [Linguagem, objetos, UTF-16, Promises e GC](2026-11-08-javascript-roadmap-item1.md).
2. [Módulos estáticos](2026-11-08-static-modules.md).
3. [BigInt e funções resumíveis](2026-11-08-bigint-resumable.md).
4. [Reflect](2026-11-08-reflect.md).
5. [Coerção de operadores e chaves](2026-11-08-operator-coercion.md).
6. [Arrays tipados BigInt e DataView](2026-11-08-bigint-typed-arrays.md).
7. [WeakRef](2026-11-08-weakref.md).
8. [WeakMap e WeakSet](2026-11-08-weak-collections.md).
9. [Math](2026-11-08-math.md).
10. [Importação dinâmica registrada](2026-11-08-dynamic-import.md).
11. [Funções numéricas](2026-11-08-numeric-globals.md).
12. [FinalizationRegistry](2026-11-08-finalization-registry.md).
13. [import.meta](2026-11-08-import-meta.md).
14. [Iteração assíncrona](2026-11-08-async-iteration.md).
15. [Async generators](2026-11-08-async-generators.md).
16. [Delegação assíncrona com yield*](2026-11-08-async-delegation.md).
17. [Literais resumíveis](2026-11-08-resumable-literals.md).
18. [Acessos e callees resumíveis](2026-11-08-resumable-access.md).
19. [Atribuições e updates resumíveis](2026-11-08-resumable-assignment.md).
20. [Delete Member resumível](2026-11-08-resumable-delete.md).

## Como interpretar as evidências

As releases vinculadas registram revisão textual e checagens estáticas de cada recorte dos itens 1–17: cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked. Isso compila fontes de testes, mas não as executa. A última checagem do item 17 foi a sessão 89428, exit 0, 1,44 s, apenas com o warning preexistente Stmt::Function; sua revisão estática final terminou sem bloqueadores. Item 18: checagem própria aprovada na sessão 8690, exit 0, 0,79 s, com cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked. Permaneceu apenas o warning preexistente Stmt::Function em parser.rs:179. Revisão textual final concluída sem bloqueadores; nenhum teste ou aplicativo foi executado.

Não foram executados testes, fixtures, Test262/WPT/fuzzing, Clippy, aplicativo/egui, render/GPU, inferência, navegação/requisições de rede, reprodução de áudio ou execução de hash/backend para validar estes recortes. Não houve medição de desempenho. Expectativas e roteiros-fonte não são observações funcionais; verificações históricas anteriores à rodada não validam o código novo.

Não houve build release, reconstrução de dist/Noir, empacotamento, commit, push ou publicação desta rodada. A distribuição existente não incorpora automaticamente as alterações do repositório. Esta página foi consolidada por leitura documental, sem executar comandos de compilação ou testes.

## Continuidade

A primeira rodada termina com os recortes acima, sem concluir os itens 1–18. A próxima rodada aprofundará capacidades centrais; este índice não antecipa entregas nem evidências dessa continuação. A execução do item 19 permanece adiada.
