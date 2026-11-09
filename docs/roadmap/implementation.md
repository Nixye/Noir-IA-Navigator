# Evolução após Noir 0.5

## Estado atual: validação e frames — 2026-11-08

A instrução posterior autoriza testes e ciclos de correção. Item 19: **validação em andamento**; itens 1–18 continuam abertos/parciais. O [baseline funcional](../validation/2026-11-08-functional-baseline.md) registra comandos e resultados novos. As seções anteriores desta rodada abaixo são históricas: o adiamento e a ausência de testes valiam naquelas entregas e não são reescritos como sucesso funcional.

O [segundo recorte do item 4](../releases/2026-11-08-animation-frames.md) acrescenta requestAnimationFrame/cancelAnimationFrame limitados, snapshot de até 32 callbacks, tempo real do host e IPC v8. A integração desktop foi liberada, com revisão consolidada sem bloqueadores e checagem estática aprovada; testes de frames e validação funcional continuam pendentes. Não equivale a event loop Web completo, vsync ou apresentação GPU. O contrato de fronteira consta no [ADR 0021](../adr/0021-host-animation-frame-pulses.md).


## Segunda rodada: Array.from e DOM real

O [novo recorte do item 1](../releases/2026-11-08-array-from-dom.md) acrescenta Array.from síncrono para iteráveis e array-like, incluindo a lista estática real de querySelectorAll por length/índices, com mapper intercalado e thisArg. O resultado é Array real; construtores alternativos são recusados. Não acrescenta iteradores DOM, novos seletores ou Array.fromAsync. Implementação parcial; revisão textual final concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Este é o único recorte JavaScript desta segunda rodada antes do avanço ao item 2. Itens 1–18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA.


## Índice da rodada de 2026-11-08

Consulte o [índice de incrementos desta rodada](../releases/2026-11-08-roadmap-first-pass.md) para os 20 incrementos JavaScript e os recortes dos itens 2–18, com referências, lacunas e evidências. Todos os itens 1–18 permanecem ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA a pedido. Não há declaração de navegador completo, validação funcional ou reconstrução de dist/Noir.

O [primeiro recorte do item 18, foco visível nos links Web](../releases/2026-11-08-web-link-keyboard-focus.md), conserva a Response nativa e desenha contorno interno quando has_focus(), independente da seleção de IA e sob clipping do viewport. Preserva interação, destino e travessia egui; não oferece ordem Tab DOM completa, árvore de acessibilidade, leitores de tela, tabindex ou eventos de foco DOM. Checagem estática do item 18 aprovada; revisão textual final concluída sem bloqueadores; sem execução de testes, aplicativo/egui ou GPU.


O Noir 0.5 possui shell com acrílico, configurações próprias, grupos/abas superiores fixáveis, favoritos, HTTP/HTTPS condicionado ao AppContainer Windows verificado, motor HTML/CSS/layout com Flex/Grid, absolute/relative e tabelas limitados, JavaScript/DOM em subconjunto e IA local com ferramentas autorizadas de navegação. Os próximos marcos ampliam compatibilidade e confiabilidade; não são promessas de prazo nem significam que as capacidades atuais sejam completas.

## 1. Navegação real consistentemente útil

Ampliar páginas de referência e registrar os casos suportados. Corrigir a diferença entre abrir a homepage Google e executar sua busca moderna. Expandir parsing, CSS, recursos, formulários e navegação com testes, preservando o motor independente.

Aceitação: destinos reais, erros claros e resultados reproduzíveis; nenhuma alegação de funcionamento baseada apenas em receber HTTP 200 ou pintar um título.

## 2. JavaScript, DOM e event loop próprios

Consolidar sessões, listeners, timers e mutações transacionais. Evoluir linguagem, memória e APIs com limites e casos selecionados de Test262/WPT. Introduzir módulos, promessas e APIs somente com implementação verificável. TypeScript exige estratégia explícita; não é equivalente ao subconjunto JavaScript atual.

Aceitação: eventos atualizam o documento correto, timers respeitam orçamento/ciclo de vida, erros não deixam mutações parciais e sessões antigas não substituem a navegação nova.

## 3. Isolamento e política de rede

O AppContainer Windows já possui provas de recusa e execução sem fallback. Ampliar testes em versões de Windows, revisar launcher/handles/Job e reduzir a superfície privilegiada de decoders e rede. Implementar isolamento Linux verificável antes de habilitar remoto. Consolidar origem, CSP/CORS, cookies, cache, certificados e armazenamento com contratos explícitos.

Aceitação: testes positivos/negativos por plataforma, falha fechada, revisão independente e nenhuma ampliação silenciosa de acesso a arquivos, rede local ou serviços de IA.

## 4. Experiência, acessibilidade e estabilidade

Validar restauração após falhas, sessões sob pressão de memória, navegação prolongada, múltiplas instâncias, teclado, leitores de tela, DPI e GPUs. Expandir acessibilidade do conteúdo. Medir desempenho e limites de renderer em condições fixas. Manter animações responsivas e redução de movimento.

Aceitação: favoritos/espaços preservados conforme preferências, ausência de processos abandonados nos cenários exercitados, erros recuperáveis e regressões visuais registradas.

## 5. IA com qualidade e privacidade verificáveis

Qwen 1.5B já executa localmente, com opções GGUF, endpoint local e provedor HTTPS. Ampliar avaliações em português, gestão de modelos, orçamentos e supervisão sob crash do processo principal. Avaliar streaming e cancelamento de requisição. As ferramentas do navegador agora usam escopo por aba/geração, permissões por ferramenta e revisão de cliques no modo granular, além de Permissão total opcional. Ampliar avaliação funcional, planejamento e resistência a prompt injection antes de tratar a automação como confiável para tarefas gerais.

Aceitação: inferência real e destino identificado, recursos verificados, nenhuma chave em texto simples, contexto anexado conscientemente e testes de prompt injection antes de ações autônomas.

## 6. Candidato público de uso geral

Concluir auditoria independente, fuzzing, ensaios prolongados, revisão de licenças, instalador, assinatura e atualização autenticada com rollback. Publicar compatibilidade e resultados de segurança do candidato.

Aceitação: validação multiplataforma e revisão independente concluídas, achados críticos resolvidos e critérios sustentados por evidências. Um pacote executável com hashes não substitui esses requisitos.

## Sequência de implementação de 2026-11-08

Na lista detalhada de [implementações planejadas](implementações%20Planejadas.md), os itens 1 (JavaScript) e 2 (WebAssembly) continuam ABERTOS/parciais. O [primeiro recorte do item 3, HTML/documentos/DOM](../releases/2026-11-08-normalized-html-documents.md), acrescenta documentos normalizados, fragmentos contextuais e getters estruturais. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Não conclui o item 3 nem altera o adiamento do item 19. A numeração desta sequência é a da lista detalhada, não a dos marcos acima.

O [primeiro recorte do item 4 da lista detalhada](../releases/2026-11-08-document-lifecycle.md) acrescenta ciclo de vida inicial e defer: readyState, readystatechange e fila DOM compartilhada com módulos sem async. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem testes/fixtures executados. Itens 1, 2, 3 e 4 ABERTOS/parciais; item 19 adiado. Não completa event loop, async, parser incremental ou carregamento Web. Não é o marco 4 de experiência/acessibilidade acima.

O [primeiro recorte do item 5 da lista detalhada](../releases/2026-11-08-programmatic-events.md) acrescenta Event/CustomEvent limitados e dispatchEvent síncrono, incluindo nós destacados. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–5 ABERTOS/parciais; item 19 adiado. Entrada, edição, ações padrão nativas, EventTarget independente e caminhos Shadow DOM continuam pendentes. Não é o marco 5 de IA acima.

O [primeiro recorte do item 6 da lista detalhada](../releases/2026-11-08-css-specificity.md) corrige especificidade lexicográfica e IDs compostos, com inline separado e precedência de !important. Implementação parcial; revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–6 ABERTOS/parciais; item 19 adiado. CSSOM, custom properties completas, CSS-wide keywords, camadas/origens completas e conformidade geral permanecem abertos. @media/@supports já existiam. Não é o marco 6 de candidato público acima.

O [primeiro recorte do item 7 da lista detalhada](../releases/2026-11-08-flex-cross-axis.md) trata de alinhamento transversal Flex row/row-reverse nowrap com altura definida, margens e offsets negativos. Stretch permanece visual e limitado; Grid, layout completo e geometria Web completa continuam abertos. Implementação parcial; revisão textual final do item 7 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–7 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 8, texto e whitespace](../releases/2026-11-08-text-whitespace.md), distingue pre-line de pre-wrap e compartilha a normalização entre fluxo e largura preferida de cada nó. O agregador de irmãos continua aproximado; não há shaping ou tipografia completos. Implementação parcial; revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–8 ABERTOS/parciais; item 19 adiado. Não corresponde a um novo marco numerado acima.

O [primeiro recorte do item 9, Content-Type HTTP](../releases/2026-11-08-http-content-type.md), acrescenta parsing limitado de MIME/charset no broker e comparação exata dos tipos textuais. Não implementa Fetch/CORS completos, sniffing, cache HTTP ou streaming. Implementação parcial; revisão textual concluída sem bloqueadores e checagem estática aprovada. Sem testes ou validação funcional. Itens 1-9 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 10, rolagem no histórico nativo](../releases/2026-11-08-history-scroll.md), conserva URL e offset CSS bidimensional por entrada, até 200 por aba, somente em memória. Push/Refresh começam no topo; Traverse aceito restaura uma vez com clamp na página exibida. Não implementa BFCache, restauração de formulários/JS, History API JavaScript ou persistência após reinício. Implementação parcial; revisão textual final do item 10 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–10 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 11, sessionStorage em memória](../releases/2026-11-08-session-storage.md), acrescenta pares reais por aba/origem HTTP(S), IPC v5 validado e quotas de 128 pares/64 KiB por origem e 16 origens por aba. Não persiste no perfil/disco nem implementa localStorage, cookies, workers ou offline. Implementação parcial; revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–11 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 12, dimensões naturais de imagens](../releases/2026-11-08-natural-image-dimensions.md), separa o tamanho CSS intrínseco da textura RGBA reduzida, com metadados obrigatórios no IPC v6. Implementação parcial; revisão estática final do item 12 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional ou visual. Itens 1-12 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 13, metadados WAV PCM locais](../releases/2026-11-08-wav-metadata.md), acrescenta audio.duration real ou NaN para audio original ativo com src relativo e preload=metadata explícito. IPC v7 transporta somente metadados limitados; não há reprodução, vídeo ou mídia remota. Implementação parcial; checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores, sem validação funcional. Itens 1–13 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 14, crypto.subtle.digest limitado](../releases/2026-11-08-bounded-crypto-digest.md), oferece SHA-256/384/512 com BufferSource real copiado e Promise intrínseca de ArrayBuffer. Exposição somente em documento top-level HTTPS confiável, sem algoritmo completo de secure contexts; cálculo síncrono limitado e callbacks no checkpoint FIFO. Quotas de 256 KiB por entrada, 2 MiB acumulados e 512 tentativas; sem RNG, chaves, encriptação, assinaturas, WebRTC, WebAuthn ou dispositivos. Implementação parcial; checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–14 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 15, codec IPC limitado](../releases/2026-11-08-bounded-ipc-codec.md), antecipa cotas de coleções durante Deserialize e limita a serialização JSON em buffer privado a 48 MiB antes de tocar o transporte. Preserva o formato e a versão 7, defaults e validação semântica/transacional posterior. Não é orçamento global de memória/CPU nem nova prova de sandbox, SOP/CSP ou política de rede. Implementação parcial; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–15 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 16, glifos recortados antes da rasterização](../releases/2026-11-08-clipped-glyph-raster.md), separa métricas e materialização no atlas. Glifos sem interseção com o clip efetivo não geram bitmap, entrada no atlas ou quad, preservando avanço e kerning. A cena mantém texto fora do viewport para rolagem; a validação global da entrada continua anterior ao recorte. Implementação parcial; checagem estática do item 16 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional, visual ou GPU nem desempenho medido. Itens 1-16 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

O [primeiro recorte do item 17, observação textual limitada](../releases/2026-11-08-bounded-ai-observation.md), limita a cópia na extração e o JSON enviado ao modelo, com filtragem prévia por permissão. Preserva escopo por aba/geração e ferramentas existentes; não implementa API completa de DOM/layout/acessibilidade/imagem. Implementação parcial; checagem estática do item 17 aprovada; revisão estática final concluída sem bloqueadores, sem testes, inferência ou validação funcional. Itens 1–17 ABERTOS/parciais; item 19 adiado. A numeração corresponde à lista detalhada, não aos marcos acima.

## Segundo ciclo: WebAssembly i64 e BigInt

O [segundo recorte do item 2](../releases/2026-11-08-webassembly-i64-bigint.md) acrescenta i64 ↔ BigInt exato nas funções exportadas stateless, sem passagem por f64. Argumentos usam ToBigInt (Number rejeitado, hint number em objetos, strings/booleanos e redução módulo 2^64); o retorno é BigInt signed. Store continua efêmero por chamada, com preflight, quotas, fuel e rollback preservados. Imports, memória/tabelas, start, globals mutáveis, segmentos e referências continuam recusados. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Não é WebAssembly completo. Itens 1-18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
## Segundo ciclo: seletores DOM/CSS compartilhados

O [segundo recorte do item 3](../releases/2026-11-08-shared-dom-selectors.md) substitui o parser simples de querySelector/querySelectorAll/matches/closest pelo parser e matcher CSS próprios. Acrescenta compostos, listas estritas, combinadores, atributos, escapes e os seletores lógicos/estruturais do subconjunto CSS existente. Mantém ordem da árvore, deduplicação, snapshot estático, closest destacado e fronteiras template/noscript. O matching pode consultar ancestrais fora da raiz; :scope, :has, namespaces, pseudoelementos, coleções vivas e Shadow DOM continuam pendentes. Limites locais e orçamento DOM compartilhado permanecem obrigatórios, inclusive em erros. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Itens 1–18 ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
