# Noir

## Estado atual: validação e frames — 2026-11-08

A instrução posterior autoriza testes e ciclos de correção. Item 19: **validação em andamento**; itens 1–18 continuam abertos/parciais. O [baseline funcional](docs/validation/2026-11-08-functional-baseline.md) registra comandos e resultados novos. As seções anteriores desta rodada abaixo são históricas: o adiamento e a ausência de testes valiam naquelas entregas e não são reescritos como sucesso funcional.

O [segundo recorte do item 4](docs/releases/2026-11-08-animation-frames.md) acrescenta requestAnimationFrame/cancelAnimationFrame limitados, snapshot de até 32 callbacks, tempo real do host e IPC v8. A integração desktop foi liberada, com revisão consolidada sem bloqueadores e checagem estática aprovada; testes de frames e validação funcional continuam pendentes. Não equivale a event loop Web completo, vsync ou apresentação GPU. O contrato de fronteira consta no [ADR 0021](docs/adr/0021-host-animation-frame-pulses.md).


## Segunda rodada: Array.from e DOM real

O [novo recorte do item 1](docs/releases/2026-11-08-array-from-dom.md) acrescenta Array.from síncrono para iteráveis e array-like, incluindo a lista estática real de querySelectorAll por length/índices, com mapper intercalado e thisArg. O resultado é Array real; construtores alternativos são recusados. Não acrescenta iteradores DOM, novos seletores ou Array.fromAsync. Implementação parcial; revisão textual final concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Este é o único recorte JavaScript desta segunda rodada antes do avanço ao item 2. Itens 1–18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA.


## Índice da rodada de 2026-11-08

Consulte o [índice de incrementos desta rodada](docs/releases/2026-11-08-roadmap-first-pass.md) para os 20 incrementos JavaScript e os recortes dos itens 2–18, com referências, lacunas e evidências. Todos os itens 1–18 permanecem ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA a pedido. Não há declaração de navegador completo, validação funcional ou reconstrução de dist/Noir.

O [primeiro recorte do item 18, foco visível nos links Web](docs/releases/2026-11-08-web-link-keyboard-focus.md), conserva a Response nativa e desenha contorno interno quando has_focus(), independente da seleção de IA e sob clipping do viewport. Preserva interação, destino e travessia egui; não oferece ordem Tab DOM completa, árvore de acessibilidade, leitores de tela, tabindex ou eventos de foco DOM. Checagem estática do item 18 aprovada; revisão textual final concluída sem bloqueadores; sem execução de testes, aplicativo/egui ou GPU.


O [primeiro recorte do item 17, observação textual limitada](docs/releases/2026-11-08-bounded-ai-observation.md), limita a cópia na extração e o JSON enviado ao modelo, com filtragem prévia por permissão. Preserva escopo por aba/geração e ferramentas existentes; não implementa API completa de DOM/layout/acessibilidade/imagem. Implementação parcial; checagem estática do item 17 aprovada; revisão estática final concluída sem bloqueadores, sem testes, inferência ou validação funcional. Itens 1–17 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 16, glifos recortados antes da rasterização](docs/releases/2026-11-08-clipped-glyph-raster.md), separa métricas e materialização no atlas. Glifos sem interseção com o clip efetivo não geram bitmap, entrada no atlas ou quad, preservando avanço e kerning. A cena mantém texto fora do viewport para rolagem; a validação global da entrada continua anterior ao recorte. Implementação parcial; checagem estática do item 16 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional, visual ou GPU nem desempenho medido. Itens 1-16 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 15, codec IPC limitado](docs/releases/2026-11-08-bounded-ipc-codec.md), antecipa cotas de coleções durante Deserialize e limita a serialização JSON em buffer privado a 48 MiB antes de tocar o transporte. Preserva o formato e a versão 7, defaults e validação semântica/transacional posterior. Não é orçamento global de memória/CPU nem nova prova de sandbox, SOP/CSP ou política de rede. Implementação parcial; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–15 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 14, crypto.subtle.digest limitado](docs/releases/2026-11-08-bounded-crypto-digest.md), oferece SHA-256/384/512 com BufferSource real copiado e Promise intrínseca de ArrayBuffer. Exposição somente em documento top-level HTTPS confiável, sem algoritmo completo de secure contexts; cálculo síncrono limitado e callbacks no checkpoint FIFO. Quotas de 256 KiB por entrada, 2 MiB acumulados e 512 tentativas; sem RNG, chaves, encriptação, assinaturas, WebRTC, WebAuthn ou dispositivos. Implementação parcial; checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–14 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 13, metadados WAV PCM locais](docs/releases/2026-11-08-wav-metadata.md), acrescenta audio.duration real ou NaN para audio original ativo com src relativo e preload=metadata explícito. IPC v7 transporta somente metadados limitados; não há reprodução, vídeo ou mídia remota. Implementação parcial; checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores, sem validação funcional. Itens 1–13 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 12, dimensões naturais de imagens](docs/releases/2026-11-08-natural-image-dimensions.md), separa o tamanho CSS intrínseco da textura RGBA reduzida, com metadados obrigatórios no IPC v6. Implementação parcial; revisão estática final do item 12 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional ou visual. Itens 1-12 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 11, sessionStorage em memória](docs/releases/2026-11-08-session-storage.md), acrescenta pares reais por aba/origem HTTP(S), IPC v5 validado e quotas de 128 pares/64 KiB por origem e 16 origens por aba. Não persiste no perfil/disco nem implementa localStorage, cookies, workers ou offline. Implementação parcial; revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–11 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 10, rolagem no histórico nativo](docs/releases/2026-11-08-history-scroll.md), conserva URL e offset CSS bidimensional por entrada, até 200 por aba, somente em memória. Push/Refresh começam no topo; Traverse aceito restaura uma vez com clamp na página exibida. Não implementa BFCache, restauração de formulários/JS, History API JavaScript ou persistência após reinício. Implementação parcial; revisão textual final do item 10 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–10 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 9, Content-Type HTTP](docs/releases/2026-11-08-http-content-type.md), acrescenta parsing limitado de MIME/charset no broker e comparação exata dos tipos textuais. Não implementa Fetch/CORS completos, sniffing, cache HTTP ou streaming. Implementação parcial; revisão textual concluída sem bloqueadores e checagem estática aprovada. Sem testes ou validação funcional. Itens 1-9 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 8, texto e whitespace](docs/releases/2026-11-08-text-whitespace.md), distingue pre-line de pre-wrap e compartilha a normalização entre fluxo e largura preferida de cada nó. O agregador de irmãos continua aproximado; não há shaping ou tipografia completos. Implementação parcial; revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–8 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 7, alinhamento transversal Flex](docs/releases/2026-11-08-flex-cross-axis.md), usa altura de conteúdo definida em row/row-reverse nowrap para center/flex-end e extensão visual limitada. Margens entram no cálculo; filhos maiores admitem deslocamento negativo. Implementação parcial; revisão textual final do item 7 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–7 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 6, especificidade CSS](docs/releases/2026-11-08-css-specificity.md), corrige a comparação lexicográfica, a contagem de ocorrências e a exigência de todos os IDs em compostos. Inline é prioridade separada e !important mantém precedência. Implementação parcial; revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–6 ABERTOS/parciais; item 19 adiado. CSSOM, variáveis completas e cascata completa continuam pendentes.

O [primeiro recorte do item 5, eventos programáticos](docs/releases/2026-11-08-programmatic-events.md), acrescenta Event/CustomEvent limitados e dispatchEvent síncrono em Window, Document e nós, inclusive destacados. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–5 ABERTOS/parciais; item 19 adiado. Não habilita ações padrão nativas, entrada/edição completas ou interfaces DOM integrais.

O [primeiro recorte do item 4, ciclo de vida inicial e defer](docs/releases/2026-11-08-document-lifecycle.md), acrescenta readyState, readystatechange e fila DOM compartilhada por módulos sem async e clássicos externos defer. Não implementa async real nem parser incremental. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem testes/fixtures executados. Itens 1, 2, 3 e 4 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 3, HTML/documentos/DOM](docs/releases/2026-11-08-normalized-html-documents.md), acrescenta normalização html/head/body, fragmentos contextuais para o setter innerHTML, getters estruturais e título DOM real. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Os itens 1, 2 e 3 permanecem ABERTOS; o item 19 continua adiado. Consulte o [subconjunto HTML/DOM](docs/specifications/html-dom-subset.md). A fixture-fonte acompanha o código sem ter sido executada.

O item 1 do roadmap, motor JavaScript próprio, permanece em andamento. O código-fonte recebeu estes incrementos em subconjuntos limitados:

- [Linguagem, objetos, UTF-16, Promises e GC](docs/releases/2026-11-08-javascript-roadmap-item1.md).
- [Módulos estáticos, ciclos e bindings vivos](docs/releases/2026-11-08-static-modules.md), com fontes autorizadas pelo broker.
- [BigInt e continuação real de async/generators](docs/releases/2026-11-08-bigint-resumable.md), incluindo loops, fechamento de iteradores e atribuições com RHS suspenso.
- [Namespace Reflect](docs/releases/2026-11-08-reflect.md), com receivers, símbolos, descritores e recusas explícitas.
- [Coerção de operadores e chaves](docs/releases/2026-11-08-operator-coercion.md), com Symbol.toPrimitive e ordem de efeitos.
- [Arrays tipados BigInt e DataView](docs/releases/2026-11-08-bigint-typed-arrays.md), com armazenamento exato de 64 bits.
- [WeakRef](docs/releases/2026-11-08-weakref.md), com referências realmente fracas a objetos do heap.
- [WeakMap e WeakSet](docs/releases/2026-11-08-weak-collections.md), com chaves fracas e marcação ephemeron.
- [Math intrínseco](docs/releases/2026-11-08-math.md), com constantes, 34 métodos e conversões observáveis.
- [Importação dinâmica registrada](docs/releases/2026-11-08-dynamic-import.md), sem novas requisições de rede ou arquivo.
- [Funções numéricas](docs/releases/2026-11-08-numeric-globals.md), com coerção observável e gramáticas UTF-16 limitadas.
- [FinalizationRegistry](docs/releases/2026-11-08-finalization-registry.md), com integração real ao GC e cleanup posterior limitado.
- [import.meta](docs/releases/2026-11-08-import-meta.md), com identidade por módulo e URL real separada do ID interno.

- [Iteração assíncrona limitada](docs/releases/2026-11-08-async-iteration.md), com for await...of, Symbol.asyncIterator e adaptação Async-from-Sync.
- [Async generators limitados](docs/releases/2026-11-08-async-generators.md), com async function*, métodos async *, fila FIFO de next/throw/return e integração com for await...of.

- [Delegação assíncrona limitada](docs/releases/2026-11-08-async-delegation.md), décimo sexto incremento em implementação parcial: yield* async com protocolo direto e fallback Async-from-Sync, sem validação funcional.

- [Literais resumíveis](docs/releases/2026-11-08-resumable-literals.md), décimo sétimo incremento em implementação parcial: arrays, holes, operandos de spread e templates com await/yield nas posições habilitadas, usando montagem e coerção compartilhadas com o avaliador comum. Revisão textual concluída e checagem estática aprovada, sem validação funcional.

- [Acessos e callees resumíveis](docs/releases/2026-11-08-resumable-access.md), décimo oitavo incremento em implementação parcial: await/yield na base/chave de leituras ordinárias e no callee de chamadas ordinárias, preservando receiver, alvo e ordem de efeitos. Revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO.

- [Atribuições e updates resumíveis](docs/releases/2026-11-08-resumable-assignment.md), décimo nono incremento do item 1 em implementação parcial: await/yield na base/chave do LHS Member ordinário, referência capturada e update com ToNumeric único. Revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. O item 19 do roadmap, testes, continua adiado.

- [Delete Member resumível](docs/releases/2026-11-08-resumable-delete.md), vigésimo incremento do item 1: await/yield na base/chave, sem ler getter do alvo, com coerção única e remoção no destino capturado. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO.

Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O item 2 agora recebe seu primeiro recorte WebAssembly, em implementação parcial, com revisão textual concluída e checagem estática aprovada e sem validação funcional. Os itens 1 e 2 permanecem ABERTOS/parciais.

O [primeiro recorte do item 2](docs/releases/2026-11-08-stateless-webassembly.md) integra wasmi 2.0.0 sem JIT, mantendo JavaScript próprio: validação binária e ponte síncrona de funções numéricas sem estado persistente, sob limites e recusas explícitas. Consulte o [ADR 0016](docs/adr/0016-bounded-stateless-webassembly.md).

Scripts clássicos externos dinâmicos agora têm carregamento real pelo broker em sessões retidas, com limites e eventos load/error ([ADR 0023](docs/adr/0023-brokered-dynamic-classic-scripts.md)). Importação dinâmica com rede, avaliação assíncrona de módulos, async/generators completos, Proxy/Reflect completos e Intl ainda faltam. Testes focados de scripts dinâmicos passaram no Linux; navegação remota ainda não foi validada nesse ambiente. O executável em `dist/Noir` não foi reconstruído. Veja os limites atuais na [matriz de capacidades](docs/architecture/capabilities.md).

Navegador independente em Rust com motor Web próprio, interface nativa, espaços de trabalho e IA local incluída. A versão **0.9.0** acrescenta capture/once/passive e propagação de eventos, funções vinculadas e aliases call/apply/bind, dimensões DOM calculadas pelo próprio layout, @supports e novos seletores CSS. A interface conserva a composição anterior. A compilação é destinada à validação posterior pelo usuário; este incremento não recebeu testes, Clippy, navegações ou validação gráfica. Google Search e YouTube continuam sem compatibilidade funcional completa declarada.

O motor não incorpora Chromium, Gecko, Servo, WebKit ou um runtime JavaScript de outro navegador. A compatibilidade ainda é parcial: a homepage do Google foi carregada pelo pipeline real, mas os resultados de busca atuais podem exigir JavaScript e redirecionamentos que o motor ainda não executa corretamente. Configurar o Google como busca padrão não equivale a compatibilidade completa com a aplicação do Google ou com sites modernos. Consulte a [matriz de capacidades](docs/architecture/capabilities.md).

A [verificação da versão 0.6](docs/validation/2026-11-03-engine-v06.md) registra 128 testes aprovados e os resultados reais de Google, YouTube e Wikipedia, incluindo os bloqueios restantes. A fixture `fixtures/compatibility-v06.html` exercita JavaScript, DOM e CSS juntos no motor.

A [correção AppContainer da 0.6.1](docs/validation/2026-11-03-sandbox-v061.md) registra a condição de pipe aberto, a coordenação com os lançamentos da IA e 133 testes aprovados, incluindo o executável gráfico release real.

A [entrega de implementação 0.7](docs/releases/2026-11-07-engine-v07.md) descreve os novos recursos, o backend gráfico e as limitações para a validação manual solicitada.

A [entrega 0.8](docs/releases/2026-11-07-engine-v08.md) trata dos erros de eval, RegExp e navigator relatados, além de ampliar CSS e recorte no driver. Os subconjuntos documentados não equivalem a conformidade completa com ECMAScript ou aplicações Web modernas.

A [entrega 0.9](docs/releases/2026-11-07-engine-v09.md) amplia eventos, chamadas e medidas do DOM apontados pelos novos logs. O pipeline de medidas e pintura usa o mesmo motor; não incorpora outro navegador ou runtime JavaScript.

## Experiência

A página inicial nativa reúne pesquisa Google e favoritos editáveis em superfícies translúcidas, tipografia proporcional e ícones vetoriais. A nova identidade usa grafite, vidro, acento limão e uma escultura procedural de linhas. O layout assimétrico reúne cartões de favoritos com título/domínio e ações para assistente e grupos. A direção visual anterior foi substituída. A preferência **Reduzir animações** interrompe o movimento decorativo. O [sistema visual](docs/architecture/visual-system.md) documenta composição e movimento.

Os grupos **Pessoal** e **Trabalho** começam disponíveis. O seletor superior alterna entre conjuntos independentes de abas. O menu de contexto permite criar e renomear grupos, fixar grupos, fixar abas e mover uma aba para outro grupo ou criar um grupo com ela. Abas fixadas ocupam ícones compactos; grupos fixados recebem atalhos superiores. Os limites atuais são oito grupos, 24 abas por grupo, 128 abas no total e 16 sessões de renderização simultâneas.

Favoritos podem ser adicionados, editados e removidos. Os destinos iniciais são Google, Wikipedia, GitHub e recursos do Noir; sua presença não garante compatibilidade completa com o site. A restauração de abas é opcional e começa desativada. Nomes e cores dos espaços, favoritos e preferências são preservados; abas fixadas são lembradas mesmo com a restauração geral desativada, pois a fixação representa uma escolha explícita de persistência. O [perfil v2](docs/architecture/workspaces-and-favorites.md) detalha migração e recuperação.

## Executar

No repositório, use **dist/Noir/noir.exe** ou o atalho **Noir** instalado na área de trabalho/menu Iniciar. O pacote portátil é **dist/Noir-windows-x64.zip**. Dentro dele, execute noir.exe mantendo resources/ai ao lado. Versão, Build ID e caminho do programa aparecem em **Configurações → Avançado**. Veja [distribuição identificada](docs/distribution.md).

O arquivo `rust-toolchain.toml` fixa o Rust e seus componentes. No Windows, são necessários Visual Studio Build Tools com desenvolvimento C++ e Windows SDK. Para preparar o modelo e o runtime de IA verificados e iniciar:

```powershell
./tools/ai/setup.ps1
cargo run -p browser-desktop
```

O pacote distribuído já inclui os recursos de IA em `resources/ai`, ao lado do executável; mantenha essa estrutura ao extraí-lo. O script não é necessário quando o pacote completo já está instalado. Os recursos não são baixados silenciosamente na inicialização.

Digite `google.com.br`, uma URL HTTP/HTTPS pública ou termos de busca na barra de endereços. Termos livres são encaminhados ao Google. Arquivos `.html` locais e URLs `file:` locais continuam disponíveis. O broker carrega recursos públicos autorizados, inclusive de CDNs, preservando os bloqueios de redes privadas e de conteúdo misto HTTPS/HTTP; o motor recebe dados, sem abrir caminhos ou URLs por conta própria.

```powershell
cargo run -p browser-desktop -- https://www.google.com.br/
cargo run -p browser-desktop -- --window-size 900x700
```

Abra o ícone de configurações ou digite `noir:settings` para ajustar aparência, transparência, restauração e IA. Diagnósticos técnicos vêm ocultos e podem ser habilitados em **Avançado**. O acrílico depende do compositor Windows e suas preferências de transparência.

O mínimo da janela é 800 × 560 pixels. `Ctrl+L` seleciona o endereço, `Ctrl+T` cria uma aba, `Ctrl+W` fecha a aba, `Ctrl+O` abre arquivo, `Ctrl+R` recarrega e `Ctrl+F` busca no documento. `Alt+←` e `Alt+→` percorrem o histórico. Zoom: 75% a 200%.

No Linux, a interface e documentos locais podem ser compilados com as dependências abaixo. A navegação remota permanece desativada até existir isolamento equivalente verificado; a CI Linux não significa validação gráfica local concluída.

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libxi-dev libgl1-mesa-dev libudev-dev
cargo run -p browser-desktop
```

## IA local e outros provedores

Use `cargo run -p browser-desktop -- --assistant` para iniciar com o painel aberto. `Ctrl+D` salva a página nos favoritos. No Linux, a configuração de credenciais também requer `libdbus-1-dev` para compilar.

O Noir inicia com **Qwen2.5-1.5B-Instruct Q4_K_M** ativo por padrão, carregado por llama.cpp em processo separado. A interface permanece utilizável durante a carga. O serviço fica em loopback autenticado e, no modo incluído, processa a conversa no dispositivo.

Nas configurações, escolha o modelo incluído, um GGUF próprio, um servidor local ou um provedor HTTPS compatível com chat/completions. Chaves ficam no cofre de credenciais do sistema, sem gravação em JSON. Na conversa normal, anexar contexto exige escolha explícita por mensagem. O modo opcional **Assistente de navegação** permite observar texto e controles, ler, abrir/alternar abas, navegar, selecionar, digitar e propor cliques. No modo granular há permissões por ferramenta, revisão de cliques e limite de oito ações por pedido. Ative **Permissão total** para conceder todas as ferramentas implementadas, inclusive cliques e formulários suportados, sem confirmações por ação e com até 32 ações por pedido. A opção pode ser revogada e não é persistida entre conversas. O modelo vê texto e controles expostos, sem visão de imagens/canvas. O modo provedor informa que mensagens seguem para o endpoint escolhido. Conversas não são persistidas no perfil.

O modelo pode errar; não há promessa de qualidade equivalente a modelos maiores. Fontes, revisões, hashes, licenças, limites e testes estão em [IA e runtime](docs/ai/runtime.md).

## CLI e inspeção visual

O CLI usa o mesmo motor em subprocesso e exporta display list e métricas. `--ppm` rasteriza em CPU, sem janela:

```powershell
cargo run -p browser-cli -- render fixtures/specimen.html --width 1024 --height 768 --output pagina.json --ppm pagina.ppm
cargo run -p browser-cli -- render https://www.google.com.br/ --output google.json --ppm google.ppm
```

A captura desktop usa um perfil de abas temporário em memória, espera o primeiro quadro renderizado, grava o framebuffer real do egui/wgpu e encerra. Requer sessão gráfica; não valida toda a navegação nem a prontidão final do modelo de IA:

```powershell
cargo run -p browser-desktop -- noir:welcome --capture "$PWD/target/validation/noir-v3-home.png"
```

A página inicial do desktop é uma composição nativa. Exportar uma fixture HTML testa o motor e não reproduz os controles da página inicial nativa.

## Desenvolvimento e validação

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
cargo test -p ai-runtime -- --ignored --nocapture
```

O último comando requer os recursos de IA e testa geração real. `scripts/test.ps1` reúne os checks normais; `scripts/build.ps1` gera release; `scripts/package.ps1` prepara desktop, CLI, recursos de IA, documentação, fixtures, licenças e `SHA256SUMS`. O manifesto detecta alterações, mas não é uma assinatura de autenticidade. A opção explícita `scripts/package.ps1 -SkipTests` permite empacotar quando a execução de testes foi dispensada; o padrão continua executando os checks. Resultados efetivamente executados ficam em `docs/validation`; o workflow remoto não deve ser apresentado como concluído antes de sua execução.

| Local | Responsabilidade |
| --- | --- |
| `apps/browser-desktop` | Interface, favoritos, espaços, abas e painel de IA |
| `apps/browser-cli` | Renderização sem janela e exportações |
| `crates/browser-core` | Broker de arquivos e HTTP/HTTPS, endereços e busca |
| `crates/browser-process`, `browser-ipc` | Sessões, ciclo de vida e mensagens versionadas |
| `crates/security-sandbox` | AppContainer Windows, limites e prova de isolamento |
| `crates/browser-config` | Perfil v2, favoritos, espaços e restauração opcional |
| `crates/ai-runtime`, `ai-context` | Inferência, configuração, credenciais e permissões |
| `crates/web-*` | DOM, HTML, CSS, layout, pintura, JavaScript e bindings próprios |

## Segurança e alcance

A navegação remota no Windows exige AppContainer sem capabilities, Job com limites e prova real de recusa de leitura particular, escrita externa, rede e criação de filhos. Uma falha bloqueia a navegação remota; não há fallback sem proteção. O [ADR 0004](docs/adr/0004-windows-sandbox-and-http-broker.md) descreve política e riscos residuais.

Isso não constitui auditoria independente ou certificação de uso geral. CSP/CORS completos, cookies, APIs Web, compatibilidade moderna, acessibilidade do conteúdo, estabilidade prolongada e distribuição assinada ainda exigem trabalho. JavaScript, DOM e timers implementam um subconjunto limitado; não oferecem conformidade integral com ECMAScript, TypeScript ou aplicações modernas. Revisão de segurança independente e resolução dos achados críticos continuam requisitos antes de um candidato público para uso geral. Veja o [roadmap](docs/roadmap/implementation.md).

## Licença

O código original está disponível sob **MIT OU Apache-2.0**: [MIT](LICENSE-MIT) e [Apache-2.0](LICENSE-APACHE). Fontes, runtime de IA, pesos e dependências mantêm suas próprias licenças e avisos; consulte [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Compatibilidade observada

As páginas iniciais reais de Google e Wikipedia foram usadas em entregas anteriores para identificar defeitos de composição. Na observação registrada, a busca Google retornava um documento de ativação/redirecionamento de JavaScript que exigia eval, classes privadas e APIs ausentes. A 0.8 acrescenta eval limitado, mas não reexecuta essa navegação nem declara a busca funcional. O relatório [Windows 0.4](docs/validation/2026-11-02-windows-v4.md) registra o resultado histórico. Não foi executada a suíte de testes neste incremento, conforme solicitação do usuário.

A revisão0.5 executou a suíte completa e navegação diagnóstica em páginas reais. O [relatório0.5](docs/validation/2026-11-02-windows-v5.md) separa falhas corrigidas, limpeza, nova interface e recursos que ainda faltam. YouTube não possui reprodução de vídeo implementada no motor. A compilação atual não deve ser confundida com compatibilidade integral de uma aplicação Web.

## Segundo ciclo: WebAssembly i64 e BigInt

O [segundo recorte do item 2](docs/releases/2026-11-08-webassembly-i64-bigint.md) acrescenta i64 ↔ BigInt exato nas funções exportadas stateless, sem passagem por f64. Argumentos usam ToBigInt (Number rejeitado, hint number em objetos, strings/booleanos e redução módulo 2^64); o retorno é BigInt signed. Store continua efêmero por chamada, com preflight, quotas, fuel e rollback preservados. Imports, memória/tabelas, start, globals mutáveis, segmentos e referências continuam recusados. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Não é WebAssembly completo. Itens 1-18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
## Segundo ciclo: seletores DOM/CSS compartilhados

O [segundo recorte do item 3](docs/releases/2026-11-08-shared-dom-selectors.md) substitui o parser simples de querySelector/querySelectorAll/matches/closest pelo parser e matcher CSS próprios. Acrescenta compostos, listas estritas, combinadores, atributos, escapes e os seletores lógicos/estruturais do subconjunto CSS existente. Mantém ordem da árvore, deduplicação, snapshot estático, closest destacado e fronteiras template/noscript. O matching pode consultar ancestrais fora da raiz; :scope, :has, namespaces, pseudoelementos, coleções vivas e Shadow DOM continuam pendentes. Limites locais e orçamento DOM compartilhado permanecem obrigatórios, inclusive em erros. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Itens 1–18 ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
