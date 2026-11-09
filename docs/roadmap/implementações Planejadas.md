# Implementações planejadas

## Estado atual: validação e frames — 2026-11-08

A instrução posterior autoriza testes e ciclos de correção. Item 19: **validação em andamento**; itens 1–18 continuam abertos/parciais. O [baseline funcional](../validation/2026-11-08-functional-baseline.md) registra comandos e resultados novos. As seções anteriores desta rodada abaixo são históricas: o adiamento e a ausência de testes valiam naquelas entregas e não são reescritos como sucesso funcional.

O [segundo recorte do item 4](../releases/2026-11-08-animation-frames.md) acrescenta requestAnimationFrame/cancelAnimationFrame limitados, snapshot de até 32 callbacks, tempo real do host e IPC v8. A integração desktop foi liberada, com revisão consolidada sem bloqueadores e checagem estática aprovada; testes de frames e validação funcional continuam pendentes. Não equivale a event loop Web completo, vsync ou apresentação GPU. O contrato de fronteira consta no [ADR 0021](../adr/0021-host-animation-frame-pulses.md).


## Segunda rodada: Array.from e DOM real

O [novo recorte do item 1](../releases/2026-11-08-array-from-dom.md) acrescenta Array.from síncrono para iteráveis e array-like, incluindo a lista estática real de querySelectorAll por length/índices, com mapper intercalado e thisArg. O resultado é Array real; construtores alternativos são recusados. Não acrescenta iteradores DOM, novos seletores ou Array.fromAsync. Implementação parcial; revisão textual final concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Este é o único recorte JavaScript desta segunda rodada antes do avanço ao item 2. Itens 1–18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA.


## Índice da rodada de 2026-11-08

Consulte o [índice de incrementos desta rodada](../releases/2026-11-08-roadmap-first-pass.md) para os 20 incrementos JavaScript e os recortes dos itens 2–18, com referências, lacunas e evidências. Todos os itens 1–18 permanecem ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA a pedido. Não há declaração de navegador completo, validação funcional ou reconstrução de dist/Noir.

O [primeiro recorte do item 18, foco visível nos links Web](../releases/2026-11-08-web-link-keyboard-focus.md), conserva a Response nativa e desenha contorno interno quando has_focus(), independente da seleção de IA e sob clipping do viewport. Preserva interação, destino e travessia egui; não oferece ordem Tab DOM completa, árvore de acessibilidade, leitores de tela, tabindex ou eventos de foco DOM. Checagem estática do item 18 aprovada; revisão textual final concluída sem bloqueadores; sem execução de testes, aplicativo/egui ou GPU.


## Andamento — 2026-11-08

- Item 17, automação e agente: O [primeiro recorte do item 17, observação textual limitada](../releases/2026-11-08-bounded-ai-observation.md), limita a cópia na extração e o JSON enviado ao modelo, com filtragem prévia por permissão. Preserva escopo por aba/geração e ferramentas existentes; não implementa API completa de DOM/layout/acessibilidade/imagem. Implementação parcial; checagem estática do item 17 aprovada; revisão estática final concluída sem bloqueadores, sem testes, inferência ou validação funcional. Itens 1–17 ABERTOS/parciais; item 19 adiado.

- Item 1, motor JavaScript próprio: em andamento. Primeiro incremento integrado ao código-fonte, sem testes ou navegação. Inclui subconjuntos de classes/desestruturação, objetos nativos, UTF-16, Promises/microtasks, reflexão e coleta de memória.
- Segundo incremento: módulos estáticos, dependências, ciclos e bindings vivos implementados em subconjunto com broker limitado. Veja [módulos estáticos](../releases/2026-11-08-static-modules.md).
- Terceiro incremento: BigInt próprio até 4096 bits, asIntN/asUintN, wrappers/protótipo e máquina real de continuação para async/await e generators em subconjunto. Veja [BigInt e funções resumíveis](../releases/2026-11-08-bigint-resumable.md).
- Quarto incremento: namespace Reflect com as 13 operações em subconjunto de objetos representáveis, incluindo receivers, símbolos, descritores e protótipos. Veja [Reflect](../releases/2026-11-08-reflect.md).
- Quinto incremento: coerção falível de operadores e chaves de propriedade com Symbol.toPrimitive, ordem de valueOf/toString, símbolos e wrappers. Veja [coerção de operadores](../releases/2026-11-08-operator-coercion.md).
- Sexto incremento: BigInt64Array/BigUint64Array e DataView BigInt com armazenamento exato, conversão, métodos, iteração e limites. Veja [arrays tipados BigInt](../releases/2026-11-08-bigint-typed-arrays.md).
- Sétimo incremento: WeakRef para objetos do heap, sem retenção forte do alvo, com limpeza antes de reciclar IDs e limites explícitos. Veja [referências fracas](../releases/2026-11-08-weakref.md).
- Oitavo incremento: WeakMap/WeakSet com chaves fracas, marcação ephemeron até ponto fixo e limites. Veja [coleções fracas](../releases/2026-11-08-weak-collections.md).
- Nono incremento: Math intrínseco com constantes, 34 métodos e coerção observável; expressões binárias/condicionais, unários seguros, RHS compostos/cadeias e operandos aninhados de await/yield ampliam as posições de suspensão sem replay. Veja [Math](../releases/2026-11-08-math.md) e [funções resumíveis](../releases/2026-11-08-bigint-resumable.md).
- Décimo incremento: importação dinâmica restrita a módulos já registrados e autorizados, com contexto lexical e avaliação/conclusão em jobs FIFO separados. Veja [importação dinâmica registrada](../releases/2026-11-08-dynamic-import.md).
- Décimo primeiro incremento: parseInt/parseFloat/isNaN/isFinite observáveis, predicados Number.is* e aliases, com gramáticas UTF-16 e orçamento. Veja [funções numéricas](../releases/2026-11-08-numeric-globals.md).
- Décimo segundo incremento: FinalizationRegistry limitado integrado ao GC, tokens fracos, held values fortes e cleanup posterior com orçamento compartilhado. Veja [finalização](../releases/2026-11-08-finalization-registry.md).
- Décimo terceiro incremento: import.meta com URL real de origem, identidade por módulo e base imutável; yield* síncrono e argumentos suspensíveis de chamadas comuns acrescentados à máquina com protocolo, receiver e resultados preservados. Veja [metadados de módulo](../releases/2026-11-08-import-meta.md) e [funções resumíveis](../releases/2026-11-08-bigint-resumable.md).
- Décimo quarto incremento: for await...of em funções async, Symbol.asyncIterator, fallback Async-from-Sync e fechamento aguardado, com limites compartilhados de continuação/Promises/jobs. Veja [iteração assíncrona](../releases/2026-11-08-async-iteration.md). O item 1 permanece aberto.
- Décimo quinto incremento: async generators limitados, com declarações/expressões/exports e métodos async *, requests FIFO, await de yield/return, integração com for await...of e rastreamento de GC. Veja [async generators](../releases/2026-11-08-async-generators.md). O item 1 permanece aberto.
- Décimo sexto incremento: yield* assíncrono limitado, em implementação parcial, com IteratorResult aguardado, value direto preservado e fallback Async-from-Sync; encaminhamento de next/throw/return, fechamento e raízes de GC. Sem validação funcional; o item 1 permanece aberto. Veja [delegação assíncrona](../releases/2026-11-08-async-delegation.md).
- Décimo sétimo incremento: literais de arrays/templates com await/yield nas posições habilitadas, holes e operandos de spread, ordem sem replay e helpers comuns/resumíveis. Coerção ToString observável e buffers com orçamento antecipado. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional. O item 1 permanece aberto. Veja [literais resumíveis](../releases/2026-11-08-resumable-literals.md).
- Décimo oitavo incremento: await/yield na base/chave de leituras Member e no callee de chamadas ordinárias, com receiver/alvo capturados, ordem de coerção e raízes de GC preservados. Implementação parcial, revisão textual concluída e checagem estática aprovada, sem validação funcional. O item 1 permanece ABERTO. Veja [acessos resumíveis](../releases/2026-11-08-resumable-access.md).
- Décimo nono incremento do item 1: await/yield na base/chave de Member ordinário como LHS de atribuição simples/composta e update ++/--. Referência capturada, ordem de coerção e ToNumeric único; RHS suspenso em Name/private/super permanece como antes. Implementação parcial, revisão textual concluída e checagem estática aprovada, sem validação funcional. O item 1 permanece ABERTO; não conclui o item 19, testes adiados. Veja [atribuições resumíveis](../releases/2026-11-08-resumable-assignment.md).
- Vigésimo incremento do item 1: delete Member ordinário resumível, sem leitura de getter, com referência única, strict/Reflect distintos e propriedades sintéticas UTF-16. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. Veja [delete resumível](../releases/2026-11-08-resumable-delete.md).
- Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O item 2 agora recebe seu primeiro recorte WebAssembly, em implementação parcial, com revisão textual concluída e checagem estática aprovada e sem validação funcional. Os itens 1 e 2 permanecem ABERTOS/parciais.
- Pendências do item 1: carregamento dinâmico sob demanda/avaliação assíncrona de módulos, ampliação das posições de suspensão e dos async generators/iteração assíncrona, APIs nativas restantes, Proxy/Reflect completos, internacionalização e lacunas de semântica/conformidade, incluindo ampliação das referências fracas/finalização.
- Item 2, WebAssembly: primeiro recorte em implementação parcial, com wasmi 2.0.0 sem JIT, validate/Module/Instance síncronos e funções numéricas sem estado persistente. Revisão textual concluída e checagem estática aprovada, sem validação funcional. Veja [contrato e limites](../releases/2026-11-08-stateless-webassembly.md).
- Item 3, HTML/documentos/DOM: [primeiro recorte](../releases/2026-11-08-normalized-html-documents.md) em implementação parcial, com html/head/body normalizados, fragmentos contextuais, getters reais e título ativo. Revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1, 2 e 3 ABERTOS/parciais.
- Item 4, event loop/ciclo de vida: [primeiro recorte](../releases/2026-11-08-document-lifecycle.md), com readyState, readystatechange, inicialização única e fila DOM de defer/módulos sem async. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem testes/fixtures executados. Itens 1, 2, 3 e 4 ABERTOS/parciais; item 19 adiado. Async real, parser incremental, rAF e ciclo de vida Web completo continuam ausentes.
- Item 5, eventos/entrada/edição: [primeiro recorte](../releases/2026-11-08-programmatic-events.md), com Event/CustomEvent limitados e dispatchEvent síncrono, incluindo nós destacados. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–5 ABERTOS/parciais; item 19 adiado. EventTarget independente, eventos especializados, ações padrão nativas e entrada/edição completas permanecem ausentes.
- Item 6, CSS/cascata: [primeiro recorte](../releases/2026-11-08-css-specificity.md), com especificidade lexicográfica, ocorrências repetidas, todos os IDs do composto exigidos e inline separado. Implementação parcial; revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–6 ABERTOS/parciais; item 19 adiado. CSSOM, custom properties completas, CSS-wide keywords, camadas/origens completas e conformidade geral continuam abertos; @media/@supports já existiam.
- Item 7, layout/geometria: [primeiro recorte](../releases/2026-11-08-flex-cross-axis.md), com alinhamento transversal Flex row/row-reverse nowrap usando altura definida, margens e offsets negativos. Stretch visual limitado; demais algoritmos e geometria Web completa continuam abertos. Implementação parcial; revisão textual final do item 7 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–7 ABERTOS/parciais; item 19 adiado.
- Item 8, texto/fontes: [primeiro recorte](../releases/2026-11-08-text-whitespace.md), com pre-line distinto de pre-wrap e normalização compartilhada entre fluxo e largura preferida por nó. Agregação de irmãos continua aproximada; fontes, fallback, shaping/bidi e tipografia completa permanecem abertos. Implementação parcial; revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–8 ABERTOS/parciais; item 19 adiado.
- Item 9, rede/recursos: O [primeiro recorte do item 9, Content-Type HTTP](../releases/2026-11-08-http-content-type.md), acrescenta parsing limitado de MIME/charset no broker e comparação exata dos tipos textuais. Não implementa Fetch/CORS completos, sniffing, cache HTTP ou streaming. Implementação parcial; revisão textual concluída sem bloqueadores e checagem estática aprovada. Sem testes ou validação funcional. Itens 1-9 ABERTOS/parciais; item 19 adiado.
- Item 10, navegação/contextos: O [primeiro recorte do item 10, rolagem no histórico nativo](../releases/2026-11-08-history-scroll.md), conserva URL e offset CSS bidimensional por entrada, até 200 por aba, somente em memória. Push/Refresh começam no topo; Traverse aceito restaura uma vez com clamp na página exibida. Não implementa BFCache, restauração de formulários/JS, History API JavaScript ou persistência após reinício. Implementação parcial; revisão textual final do item 10 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–10 ABERTOS/parciais; item 19 adiado.
- Item 11, persistência/workers/offline: O [primeiro recorte do item 11, sessionStorage em memória](../releases/2026-11-08-session-storage.md), acrescenta pares reais por aba/origem HTTP(S), IPC v5 validado e quotas de 128 pares/64 KiB por origem e 16 origens por aba. Não persiste no perfil/disco nem implementa localStorage, cookies, workers ou offline. Implementação parcial; revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–11 ABERTOS/parciais; item 19 adiado.
- Item 12, imagens e gráficos Web: O [primeiro recorte do item 12, dimensões naturais de imagens](../releases/2026-11-08-natural-image-dimensions.md), separa o tamanho CSS intrínseco da textura RGBA reduzida, com metadados obrigatórios no IPC v6. Implementação parcial; revisão estática final do item 12 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional ou visual. Itens 1-12 ABERTOS/parciais; item 19 adiado.
- Item 13, áudio/vídeo/streaming: O [primeiro recorte do item 13, metadados WAV PCM locais](../releases/2026-11-08-wav-metadata.md), acrescenta audio.duration real ou NaN para audio original ativo com src relativo e preload=metadata explícito. IPC v7 transporta somente metadados limitados; não há reprodução, vídeo ou mídia remota. Implementação parcial; checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores, sem validação funcional. Itens 1–13 ABERTOS/parciais; item 19 adiado.
- Item 14, APIs de segurança e dispositivos: O [primeiro recorte do item 14, crypto.subtle.digest limitado](../releases/2026-11-08-bounded-crypto-digest.md), oferece SHA-256/384/512 com BufferSource real copiado e Promise intrínseca de ArrayBuffer. Exposição somente em documento top-level HTTPS confiável, sem algoritmo completo de secure contexts; cálculo síncrono limitado e callbacks no checkpoint FIFO. Quotas de 256 KiB por entrada, 2 MiB acumulados e 512 tentativas; sem RNG, chaves, encriptação, assinaturas, WebRTC, WebAuthn ou dispositivos. Implementação parcial; checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–14 ABERTOS/parciais; item 19 adiado.
- Item 15, segurança e isolamento: O [primeiro recorte do item 15, codec IPC limitado](../releases/2026-11-08-bounded-ipc-codec.md), antecipa cotas de coleções durante Deserialize e limita a serialização JSON em buffer privado a 48 MiB antes de tocar o transporte. Preserva o formato e a versão 7, defaults e validação semântica/transacional posterior. Não é orçamento global de memória/CPU nem nova prova de sandbox, SOP/CSP ou política de rede. Implementação parcial; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–15 ABERTOS/parciais; item 19 adiado.
- Item 16, driver próprio: O [primeiro recorte do item 16, glifos recortados antes da rasterização](../releases/2026-11-08-clipped-glyph-raster.md), separa métricas e materialização no atlas. Glifos sem interseção com o clip efetivo não geram bitmap, entrada no atlas ou quad, preservando avanço e kerning. A cena mantém texto fora do viewport para rolagem; a validação global da entrada continua anterior ao recorte. Implementação parcial; checagem estática do item 16 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional, visual ou GPU nem desempenho medido. Itens 1-16 ABERTOS/parciais; item 19 adiado.
- Itens 2 a 18: não concluídos por este recorte; a presença de bases anteriores não equivale a conclusão. As lacunas do item 1 continuam abertas.
- Item 19: execução de testes e comprovação de compatibilidade adiadas conforme solicitação. Checagem de compilação não é validação funcional.
- O executável em dist/Noir não foi reconstruído. Não houve publicação.

Detalhes e limites: [primeiro incremento do item 1](../releases/2026-11-08-javascript-roadmap-item1.md).

## Escopo de referência original

1\. \*\*Motor JavaScript próprio\*\*

&#x20;  - Sintaxe e semântica ECMAScript: expressões, declarações, funções, classes, herança, campos privados, destructuring, templates, spread/rest e iteração.

&#x20;  - Escopos, closures, hoisting, TDZ, modo estrito, coerções e tratamento de exceções.

&#x20;  - Módulos, dependências, ciclos, bindings vivos, importação dinâmica e avaliação assíncrona.

&#x20;  - Promises, `async`/`await`, generators e iteradores síncronos/assíncronos.

&#x20;  - Objetos nativos completos: arrays, strings, números, datas, coleções, símbolos, BigInt, buffers, arrays tipados, proxies e reflexão.

&#x20;  - JSON, expressões regulares, Unicode e UTF-16 corretos.

&#x20;  - Garbage collector, referências fracas, finalização e integração segura com objetos DOM.

&#x20;  - Internacionalização: números, datas, calendários, fusos, ordenação e segmentação de texto.



&#x20;  Referências: \[ECMA-262](https://tc39.es/ecma262/) e \[ECMA-402](https://tc39.es/ecma402/).



2\. \*\*WebAssembly\*\*

&#x20;  - Validação, instanciação e execução de módulos.

&#x20;  - Memórias, tabelas, imports, exports e integração com JavaScript.

&#x20;  - Compilação/carregamento por streaming.

&#x20;  - Extensões estabilizadas exigidas pelas aplicações atendidas.



&#x20;  WebAssembly é uma plataforma separada de JavaScript e precisa de implementação própria ou de uma biblioteca permitida pelo projeto. \[Especificações oficiais](https://webassembly.github.io/spec/).



3\. \*\*HTML, documentos e DOM\*\*

&#x20;  - Parser HTML completo, incluindo recuperação padronizada de documentos malformados.

&#x20;  - Construção correta da árvore, modos de documento e compatibilidade legada.

&#x20;  - Elementos HTML, atributos, propriedades refletidas e comportamento de cada elemento.

&#x20;  - Namespaces, SVG integrado ao documento e MathML.

&#x20;  - Criação, movimentação, clonagem, adoção e remoção de nós.

&#x20;  - Fragmentos, templates, coleções vivas, consultas e seletores.

&#x20;  - Web Components: custom elements, Shadow DOM, slots e ciclos de vida.

&#x20;  - Observadores de mutação, tamanho e interseção.

&#x20;  - Interfaces e conversões Web IDL, incluindo protótipos e descritores corretos.



&#x20;  Referências: \[HTML Standard](https://html.spec.whatwg.org/multipage/) e \[DOM Standard](https://dom.spec.whatwg.org/).



4\. \*\*Event loop e ciclo de vida\*\*

&#x20;  - Tarefas, microtasks e checkpoints com ordenação correta.

&#x20;  - Timers, frames de animação e sincronização com renderização.

&#x20;  - Scripts clássicos, módulos, `async`, `defer` e scripts inseridos dinamicamente.

&#x20;  - Estados de carregamento, eventos de sucesso/falha e cancelamento.

&#x20;  - Visibilidade, congelamento, retomada e descarte de páginas.

&#x20;  - Coordenação entre documentos, workers e processos.



5\. \*\*Eventos, entrada e edição\*\*

&#x20;  - Captura, alvo, propagação, cancelamento e ações padrão.

&#x20;  - Event, CustomEvent, EventTarget e eventos especializados.

&#x20;  - Teclado, mouse, ponteiro, toque, wheel, gestos e composição de texto.

&#x20;  - Foco, seleção, ranges, edição e `contenteditable`.

&#x20;  - Clipboard, drag-and-drop e integração com métodos de entrada do sistema.

&#x20;  - Caminhos compostos de eventos em Shadow DOM.

&#x20;  - AbortController/AbortSignal integrados às operações canceláveis.



6\. \*\*CSS e cascata\*\*

&#x20;  - Parsing, seletores, especificidade, herança e cascata completos.

&#x20;  - Camadas, escopos, variáveis, funções, unidades e valores computados.

&#x20;  - Pseudoclasses, pseudo-elementos e conteúdo gerado.

&#x20;  - Media queries, feature queries e container queries.

&#x20;  - Fontes, cores, fundos, bordas, sombras, máscaras e filtros.

&#x20;  - Transforms, transições, animações e view transitions.

&#x20;  - Propriedades lógicas, escrita vertical e direção de texto.

&#x20;  - CSSOM, folhas dinâmicas e consulta de estilos computados.



&#x20;  CSS é composto por diversos módulos; o \[CSS Snapshot 2026](https://www.w3.org/TR/css-2026/) organiza o conjunto de especificações por estabilidade.



7\. \*\*Layout e geometria\*\*

&#x20;  - Fluxos block/inline e formação de linhas.

&#x20;  - Dimensionamento intrínseco, margens, padding, bordas e restrições de tamanho.

&#x20;  - Flexbox, Grid, tabelas, floats, colunas e fragmentação.

&#x20;  - Posicionamento static/relative/absolute/fixed/sticky.

&#x20;  - Containing blocks, stacking contexts, `z-index` e ordem de pintura.

&#x20;  - Overflow, scroll containers, scroll snap e ancoragem de rolagem.

&#x20;  - Geometria correta para zoom, DPI e transforms.

&#x20;  - `getBoundingClientRect`, medidas client/offset/scroll e hit testing.

&#x20;  - Invalidação e recálculo incremental quando DOM ou estilos mudarem.



8\. \*\*Texto e fontes\*\*

&#x20;  - Carregamento e seleção de fontes locais e externas.

&#x20;  - Shaping, kerning, ligaturas, fontes variáveis e fallback.

&#x20;  - Unicode, escrita bidirecional e diferentes sistemas de escrita.

&#x20;  - Quebra de linha, hifenização, justificação e decoração.

&#x20;  - Emojis, símbolos e fontes coloridas.

&#x20;  - Consistência entre medidas, seleção, hit testing e pintura.



9\. \*\*Rede e carregamento de recursos\*\*

&#x20;  - URLs, resolução relativa, codificações e identificação de conteúdo.

&#x20;  - DNS, conexões, TLS, certificados e protocolos HTTP compatíveis.

&#x20;  - Redirects, autenticação, cookies e credenciais.

&#x20;  - Cache, revalidação, compressão e reutilização de conexões.

&#x20;  - Fetch, XMLHttpRequest, requests, responses, headers e streams.

&#x20;  - Recursos dinâmicos, prioridades, carregamento progressivo e cancelamento.

&#x20;  - WebSocket, eventos enviados pelo servidor e transportes adicionais conforme o escopo.

&#x20;  - Políticas de origem, CORS, conteúdo misto e integridade de recursos.



&#x20;  O comportamento de requisições e respostas precisa seguir o \[Fetch Standard](https://fetch.spec.whatwg.org/).



10\. \*\*Navegação e contextos de páginas\*\*

&#x20;   - Location, History API, fragmentos e navegação entre documentos.

&#x20;   - Histórico de sessão e restauração de estado.

&#x20;   - Iframes, popups, contextos aninhados e comunicação entre documentos.

&#x20;   - `postMessage`, MessageChannel e structured clone.

&#x20;   - Formulários GET/POST, multipart, upload e validação.

&#x20;   - Downloads, redirecionamentos e protocolos externos autorizados.

&#x20;   - Navegação transacional: respostas antigas nunca substituem a navegação atual.



11\. \*\*Persistência, workers e aplicações offline\*\*

&#x20;   - Cookies, localStorage e sessionStorage.

&#x20;   - IndexedDB, Cache Storage e quotas.

&#x20;   - Particionamento por origem e políticas de descarte.

&#x20;   - Dedicated Workers, Shared Workers e Service Workers.

&#x20;   - Atualização, ativação e encerramento correto desses workers.

&#x20;   - Interceptação de requisições e funcionamento offline.

&#x20;   - Worklets e APIs de execução especializadas quando necessárias.



12\. \*\*Imagens e gráficos Web\*\*

&#x20;   - Formatos de imagem utilizados pelos sites, incluindo animação.

&#x20;   - SVG integrado, estilos, texto, filtros, máscaras e referências.

&#x20;   - Imagens responsivas, seleção por resolução e carregamento progressivo.

&#x20;   - Canvas 2D, OffscreenCanvas e ImageBitmap.

&#x20;   - WebGL e WebGPU para aplicações que dependem dessas APIs.

&#x20;   - Gestão de memória gráfica e recuperação após perda de contexto.



&#x20;   Expor WebGPU à página é um requisito distinto de usar GPU internamente para pintar o navegador. \[Especificação WebGPU](https://www.w3.org/TR/webgpu/).



13\. \*\*Áudio, vídeo e streaming\*\*

&#x20;   - Elementos de mídia com estados, métodos e eventos corretos.

&#x20;   - Contêineres, demultiplexação e codecs.

&#x20;   - Sincronização de áudio/vídeo, buffering, busca temporal e reprodução adaptativa.

&#x20;   - Media Source Extensions, Web Audio e WebCodecs.

&#x20;   - Legendas, trilhas, fullscreen, picture-in-picture e Media Session.

&#x20;   - Captura de câmera, microfone e tela.

&#x20;   - Decodificação por hardware quando disponível.

&#x20;   - Encrypted Media Extensions e integração com sistemas de descriptografia para serviços que os exigem.



&#x20;   EME define a interface de integração; ela não fornece, por si só, um sistema de DRM. \[Encrypted Media Extensions](https://www.w3.org/TR/encrypted-media/).



14\. \*\*Comunicação, identidade e dispositivos\*\*

&#x20;   - WebRTC e comunicação de áudio, vídeo e dados em tempo real.

&#x20;   - Web Crypto e geração segura de aleatoriedade.

&#x20;   - Autenticação, credenciais, WebAuthn e passkeys.

&#x20;   - Permissões, notificações e geolocalização.

&#x20;   - APIs de dispositivos utilizadas pelas páginas atendidas: gamepads, MIDI, USB, Bluetooth, serial e outras.

&#x20;   - Integração com o sistema operacional e indicação correta de capacidades indisponíveis.



&#x20;   Essas APIs são condicionais: dependem da página, do sistema, do hardware e das permissões concedidas.



15\. \*\*Segurança e isolamento\*\*

&#x20;   - Separação entre interface, broker, renderers, GPU e decodificadores.

&#x20;   - Sandbox efetiva e isolamento entre origens/contextos.

&#x20;   - Validação de IPC e limites de memória, CPU, profundidade e trabalho.

&#x20;   - Same-origin policy, CSP e políticas de isolamento entre origens.

&#x20;   - Proteção de credenciais, armazenamento e permissões.

&#x20;   - Gestão de certificados e atualização de componentes vulneráveis.

&#x20;   - Tratamento seguro de crashes e conteúdo malformado.

&#x20;   - Acessos privilegiados mediados pelo broker, preservando o motor sem acesso direto a arquivos e rede.



16\. \*\*Driver próprio de renderização do Noir\*\*

&#x20;   - \*\*Cena e display list próprias\*\*, produzidas pelo layout.

&#x20;   - Árvores de propriedades para transforms, efeitos, clips e rolagem.

&#x20;   - Rasterização de texto, imagens, vetores, bordas e efeitos.

&#x20;   - Atlas de glifos, texturas, buffers e gerenciamento de recursos.

&#x20;   - Composição de camadas, superfícies intermediárias e transparência.

&#x20;   - Invalidação parcial, tiles, descarte de áreas invisíveis e controle de sobreposição.

&#x20;   - Agendamento de frames e sincronização com apresentação.

&#x20;   - Scroll e animações responsivos durante trabalho pesado da página.

&#x20;   - Integração de canvas, conteúdo 3D e vídeo.

&#x20;   - Backend GPU, fallback CPU e recuperação após perda do dispositivo.

&#x20;   - Hit testing consistente com o conteúdo efetivamente apresentado.



&#x20;   Esse driver pode ser código próprio \*\*em espaço de usuário\*\*, utilizando APIs gráficas do sistema. Criar um driver de kernel para a placa de vídeo seria outro projeto. A separação entre pintura, rasterização e composição aparece também em arquiteturas de navegadores estabelecidos. \[Referência de arquitetura](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/how\_cc\_works.md).



17\. \*\*Automação e agente\*\*

&#x20;   - API interna estável para observar DOM, layout, acessibilidade e imagem renderizada.

&#x20;   - Navegação, seleção, digitação, rolagem e cliques com identificação de contexto.

&#x20;   - Esperas por carregamento, elementos, alterações e resultados.

&#x20;   - Cancelamento, rastreamento de ações e rejeição de comandos antigos.

&#x20;   - Permissões e proteção contra instruções maliciosas vindas das páginas.

&#x20;   - WebDriver/WebDriver BiDi se houver objetivo de integração com ferramentas externas.



&#x20;   \*\*WebDriver é um protocolo de automação; é diferente do driver gráfico.\*\* \[Definição oficial](https://www.w3.org/TR/webdriver2/).



18\. \*\*Produto, acessibilidade e operação\*\*

&#x20;   - Árvore de acessibilidade do conteúdo e integração com leitores de tela.

&#x20;   - Navegação por teclado, foco e preferências de acessibilidade.

&#x20;   - Perfis, histórico, downloads, impressão e recuperação de sessões.

&#x20;   - Gestão de processos, abas suspensas e pressão de memória.

&#x20;   - Diagnósticos de console, rede, DOM, estilos e desempenho.

&#x20;   - Instalador, assinatura, atualização autenticada e rollback.

&#x20;   - Compatibilidade com os sistemas operacionais e GPUs oficialmente atendidos.



19\. \*\*Comprovação de compatibilidade\*\*

&#x20;   - Conformidade JavaScript acompanhada pelo Test262.

&#x20;   - Conformidade da plataforma acompanhada pelos Web Platform Tests.

&#x20;   - Suítes específicas de gráficos e mídia.

&#x20;   - Regressões com páginas reais, incluindo busca, login, formulários e reprodução.

&#x20;   - Fuzzing, testes prolongados, desempenho e revisão de segurança.

&#x20;   - Matriz pública distinguindo implementado, parcial, indisponível e validado.



&#x20;   \[Test262](https://github.com/tc39/test262) e \[Web Platform Tests](https://web-platform-tests.org/) fornecem as principais bases de conformidade. Você pode adiar essa execução durante a implementação, mas a afirmação de compatibilidade precisa dessas evidências.



\*\*O escopo de referência do Noir deve ser: implementação própria desses subsistemas, cobertura rastreável das especificações selecionadas e manutenção contínua de compatibilidade.\*\* A aparência da interface, a IA e o driver gráfico são partes do produto; nenhuma delas substitui a implementação correta da plataforma Web.

## Segundo ciclo: WebAssembly i64 e BigInt

O [segundo recorte do item 2](../releases/2026-11-08-webassembly-i64-bigint.md) acrescenta i64 ↔ BigInt exato nas funções exportadas stateless, sem passagem por f64. Argumentos usam ToBigInt (Number rejeitado, hint number em objetos, strings/booleanos e redução módulo 2^64); o retorno é BigInt signed. Store continua efêmero por chamada, com preflight, quotas, fuel e rollback preservados. Imports, memória/tabelas, start, globals mutáveis, segmentos e referências continuam recusados. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Não é WebAssembly completo. Itens 1-18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
## Segundo ciclo: seletores DOM/CSS compartilhados

O [segundo recorte do item 3](../releases/2026-11-08-shared-dom-selectors.md) substitui o parser simples de querySelector/querySelectorAll/matches/closest pelo parser e matcher CSS próprios. Acrescenta compostos, listas estritas, combinadores, atributos, escapes e os seletores lógicos/estruturais do subconjunto CSS existente. Mantém ordem da árvore, deduplicação, snapshot estático, closest destacado e fronteiras template/noscript. O matching pode consultar ancestrais fora da raiz; :scope, :has, namespaces, pseudoelementos, coleções vivas e Shadow DOM continuam pendentes. Limites locais e orçamento DOM compartilhado permanecem obrigatórios, inclusive em erros. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Itens 1–18 ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
