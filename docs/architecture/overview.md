# Arquitetura implementada

## Estado atual: validação e frames — 2026-11-08

A instrução posterior autoriza testes e ciclos de correção. Item 19: **validação em andamento**; itens 1–18 continuam abertos/parciais. O [baseline funcional](../validation/2026-11-08-functional-baseline.md) registra comandos e resultados novos. As seções anteriores desta rodada abaixo são históricas: o adiamento e a ausência de testes valiam naquelas entregas e não são reescritos como sucesso funcional.

O [segundo recorte do item 4](../releases/2026-11-08-animation-frames.md) acrescenta requestAnimationFrame/cancelAnimationFrame limitados, snapshot de até 32 callbacks, tempo real do host e IPC v8. A integração desktop foi liberada, com revisão consolidada sem bloqueadores e checagem estática aprovada; testes de frames e validação funcional continuam pendentes. Não equivale a event loop Web completo, vsync ou apresentação GPU. O contrato de fronteira consta no [ADR 0021](../adr/0021-host-animation-frame-pulses.md).


## Segunda rodada: Array.from e DOM real

O [novo recorte do item 1](../releases/2026-11-08-array-from-dom.md) acrescenta Array.from síncrono para iteráveis e array-like, incluindo a lista estática real de querySelectorAll por length/índices, com mapper intercalado e thisArg. O resultado é Array real; construtores alternativos são recusados. Não acrescenta iteradores DOM, novos seletores ou Array.fromAsync. Implementação parcial; revisão textual final concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Este é o único recorte JavaScript desta segunda rodada antes do avanço ao item 2. Itens 1–18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA.


## Índice da rodada de 2026-11-08

Consulte o [índice de incrementos desta rodada](../releases/2026-11-08-roadmap-first-pass.md) para os 20 incrementos JavaScript e os recortes dos itens 2–18, com referências, lacunas e evidências. Todos os itens 1–18 permanecem ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA a pedido. Não há declaração de navegador completo, validação funcional ou reconstrução de dist/Noir.

O [primeiro recorte do item 18, foco visível nos links Web](../releases/2026-11-08-web-link-keyboard-focus.md), conserva a Response nativa e desenha contorno interno quando has_focus(), independente da seleção de IA e sob clipping do viewport. Preserva interação, destino e travessia egui; não oferece ordem Tab DOM completa, árvore de acessibilidade, leitores de tela, tabindex ou eventos de foco DOM. Checagem estática do item 18 aprovada; revisão textual final concluída sem bloqueadores; sem execução de testes, aplicativo/egui ou GPU.


O [primeiro recorte do item 16, glifos recortados antes da rasterização](../releases/2026-11-08-clipped-glyph-raster.md), separa métricas e materialização no atlas. Glifos sem interseção com o clip efetivo não geram bitmap, entrada no atlas ou quad, preservando avanço e kerning. A cena mantém texto fora do viewport para rolagem; a validação global da entrada continua anterior ao recorte. Implementação parcial; checagem estática do item 16 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional, visual ou GPU nem desempenho medido. Itens 1-16 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 15, codec IPC limitado](../releases/2026-11-08-bounded-ipc-codec.md), antecipa cotas de coleções durante Deserialize e limita a serialização JSON em buffer privado a 48 MiB antes de tocar o transporte. Preserva o formato e a versão 7, defaults e validação semântica/transacional posterior. Não é orçamento global de memória/CPU nem nova prova de sandbox, SOP/CSP ou política de rede. Implementação parcial; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–15 ABERTOS/parciais; item 19 adiado.

O [primeiro recorte do item 14, crypto.subtle.digest limitado](../releases/2026-11-08-bounded-crypto-digest.md), oferece SHA-256/384/512 com BufferSource real copiado e Promise intrínseca de ArrayBuffer. Exposição somente em documento top-level HTTPS confiável, sem algoritmo completo de secure contexts; cálculo síncrono limitado e callbacks no checkpoint FIFO. Quotas de 256 KiB por entrada, 2 MiB acumulados e 512 tentativas; sem RNG, chaves, encriptação, assinaturas, WebRTC, WebAuthn ou dispositivos. Implementação parcial; checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional. Itens 1–14 ABERTOS/parciais; item 19 adiado.

## Fluxo de uma navegação

1. A interface recebe um endereço explícito do usuário ou um destino de link.
2. O broker normaliza domínios para HTTPS e consultas para busca Google; carrega páginas internas, HTML local autorizado ou HTTP/HTTPS público com limites. Rede exige antes uma prova AppContainer para o executável corrente.
3. Recursos locais são confinados ao diretório canônico. Recursos HTTP subordinados podem vir de outras origens públicas autorizadas, sem credenciais e sem conteúdo misto HTTPS/HTTP, com DNS público fixado no transporte, redirects revalidados e limites de bytes/contagem/prazo.
4. O supervisor inicia o próprio executável com pipes dedicados. Documentos remotos Windows exigem AppContainer sem capabilities e Job com um processo, 512 MiB e kill-on-close; não há fallback sem isolamento. Linux recusa remoto até haver backend equivalente verificado.
5. IPC v7 transporta documento, imagens, metadados WAV locais, folhas de estilo, scripts autorizados e viewport como dados. O motor não recebe capacidades de arquivos, rede, credenciais ou serviços de IA.
6. HTML → DOM → VM JavaScript própria/bindings → CSS → layout → display list executam no filho.
7. O pai valida geometrias, controles, recursos e versão/id antes de entregar a saída à interface. Operações de sessão têm deadline de dez segundos.
8. A interface pinta itens e controles nativos e entrega eventos explícitos. O CLI usa um worker descartável para JSON e rasterização CPU em PPM.

O desktop retém a VM e o DOM em uma sessão por documento, com no máximo 16 processos ativos. Reflow, cliques e timers usam o mesmo worker e não repetem scripts de inicialização. Navegar ou fechar a aba elimina a sessão; ela não pode trocar de documento/origem por outro `Begin`. Valores digitados são pares tipados de ID/texto e nunca são interpolados em código. O perfil permite até oito workspaces, 24 abas por grupo, 128 abas no total e 64 favoritos; abas ainda não abertas não precisam de processo.

O protocolo `Tick` permite avanço de relógio de até 60 segundos por operação. O desktop entrega o tempo segundo sua política de aba ativa; abas estacionadas são limitadas. Isso não equivale ao event loop completo com todas as filas de tarefas Web.

## Codec IPC limitado — item 15

O [ADR 0020](../adr/0020-bounded-ipc-codec.md) limita coleções durante Deserialize sem confiar em size_hint: items/links 100.000, warnings 128, options 256 e snapshots/seeds de storage 128 pares. O visitor de items também verifica 256 controles após construir cada item; o controle excedente não é retido. Ao atingir a cota total de uma coleção, N+1 é recusado sem desserialização. Os defaults e o wire v7 são preservados.

O writer acumula JSON em buffer privado de até 48 MiB, com crescimento solicitado limitado. Somente após sucesso escreve u32 LE, corpo e flush; falha de serialização/cota não toca o transporte, enquanto falha de I/O pode deixar envio parcial. O frame recebido e strings ainda podem alocar antes de validações menores; não há teto global de heap/CPU. O supervisor continua validando versão/ID, resultado, página inteira, acesso e pares antes de aceitar o checkpoint. Não se modifica NSP1, autoridade ou política de sandbox/rede. Checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores, sem validação funcional.

## Perfil e contexto de IA

`browser-config` mantém preferências, workspaces e favoritos locais. Restauração geral de abas é opt-in; abas explicitamente fixadas são preservadas; o perfil não recebe HTML, imagens, credenciais ou contexto de IA. O serviço de inferência é separado da renderização; backend/modelo local e provedores externos têm configuração própria. Credenciais ficam no subsistema de credenciais. `ai-context` identifica permissões por aba e geração; no modo granular, confirmações têm uso único; Permissão total concede antecipadamente as ferramentas implementadas e dispensa revisão por ação. A página nunca concede a si mesma acesso ao serviço de IA. O broker HTTP recusa loopback/redes privadas, inclusive o endpoint de IA da máquina.

## Responsabilidades

| Componente | Pode fazer | Não deve fazer |
| --- | --- | --- |
| Shell nativa | Entrada, abas, histórico por aba, pintura e métricas | Interpretar scripts de páginas ou conceder permissões a partir do conteúdo |
| Broker | Ler HTML autorizado, carregar HTTP/TLS e recursos limitados | Conceder acesso arbitrário por links ou misturar autoridade local/remota |
| Supervisor de processos | Criar, comunicar, impor deadline e encerrar workers | Aceitar geometria ou identificadores de recurso sem validação |
| HTML/DOM | Construir e consultar a árvore do documento | Acessar serviços privilegiados |
| CSS | Seletores, cascata e estilos do subconjunto implementado | Interpretar regras não suportadas como capacidades completas |
| Layout | Produzir geometria e fragmentos de texto | Abrir arquivos ou depender da janela nativa |
| Pintura | Consumir retângulos, texto e imagens | Executar ações de navegação sem passar pelo broker |
| Serviço de IA | Inferência/configuração separadas e ferramentas autorizadas | Tratar texto da página como instruções privilegiadas |

## Contrato IPC v7

O frame tem um comprimento `u32` little-endian seguido de JSON UTF-8. Comprimentos nulos, truncados ou maiores que 48 MiB são rejeitados. As mensagens contêm versão e identificador de requisição; o pai exige correspondência na resposta. Os tipos são definidos em `browser-ipc` e usam apenas dados serializáveis.

Uma mudança incompatível exige uma nova versão. Novos campos devem ter limites e semântica definidos antes de entrar no contrato. O teto da mensagem vale também depois da serialização: recursos que cabem em seus limites individuais podem exceder o frame em JSON e precisam ser rejeitados.

`DocumentInput` inclui scripts autorizados `ScriptAsset {src, source}`. `RenderedPage` inclui controles tipados e o próximo prazo de timer. Sessões começam com `Begin` e aceitam `Reflow`, `Event` e `Tick`, com IDs explícitos e framing limitado. Resposta tardia ou inválida encerra o worker. Veja o [ADR 0004](../adr/0004-windows-sandbox-and-http-broker.md) para a fronteira do SO.

## Limites da fundação

O runtime JS é um subconjunto próprio, com sintaxe, DOM, callbacks e timers limitados. Não há compatibilidade ECMAScript/TypeScript universal, módulos modernos completos, composição incremental, carregamento assíncrono completo, layout Flexbox/Grid, Fetch/CORS/CSP de navegador, workers, IndexedDB ou cookies/cache completos. Redimensionar recalcula a pintura sem repetir os scripts. O compositor nativo e o rasterizador CPU não representam um pipeline GPU Web completo. Carregar a homepage do Google não demonstra funcionamento integral da sua aplicação de busca JS.

A integração do sistema operacional usa as facilidades da biblioteca de janela. A execução em Linux é alvo de CI, mas um build bem-sucedido não substitui a inspeção em uma sessão gráfica real de cada plataforma.

A autorização de recursos públicos e o transporte de folhas em ordem DOM seguem o ADR0008. O modo total não retira limites do broker ou isolamento do renderer. Exceções ordinárias preservam efeitos de script já realizados; esgotamento de recursos restaura a operação e desativa scripts conforme ADR0007.

## WebAssembly: fronteira do primeiro recorte do item 2

Em implementação parcial, revisão textual concluída e checagem estática aprovada, sem validação funcional. web-javascript delega validação/compilação e interpretação binária ao novo web-wasm, que encapsula wasmi 2.0.0 sem JIT. Não há browser engine incorporado: JavaScript continua próprio. Nenhum novo canal IPC ou acesso a arquivos/rede é concedido ao motor.

O Runtime clonável precisa restaurar VM/DOM em Resource (ADR 0007), mas Store não é clonável. Module/Engine imutáveis são compartilháveis; Instance conserva identidade JS e só executa com Store temporário após provar ausência de estado mutável persistente. Imports, memórias, tabelas, start e globals mutáveis são recusados. Funções exportadas mantêm a instância alcançável; registros inacessíveis são varridos pelo GC. Fuel e quotas compartilham os limites da VM, com consumo preservado em trap/rollback. Estado WASM futuro exige snapshot integral ou revisão arquitetural, não Arc/Mutex<Store> entre snapshots. Veja o [ADR 0016](../adr/0016-bounded-stateless-webassembly.md).

## HTML normalizado: primeiro recorte do item 3

O parser de documento constrói html/head/body únicos; fragmentos seguem entrada contextual separada, sem wrappers nem troca do modo receptor. Bindings consultam estrutura real, distinguem título DOM vazio do fallback visual e preparam a substituição innerHTML antes de alterar filhos. CSS/layout passam a consumir wrappers reais (:root, herança e margem body de 8 px), sem promessa de manter pixels anteriores. Template/noscript conservam inatividade durante recuperação e coleta de recursos. Nenhum novo I/O, IPC ou acesso à rede é concedido. Construção WHATWG completa e namespaces continuam pendentes. Revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Veja [contrato e limites](../releases/2026-11-08-normalized-html-documents.md).

## Ciclo de vida inicial: primeiro recorte do item 4

A sessão passa por loading, interactive e complete e executa run_initial uma única vez. DOM completo e assets autorizados já existem antes dos clássicos ordinários. A fila DOM única de módulos sem async e clássicos externos defer segue interactive/readystatechange; depois vêm DOMContentLoaded, complete/readystatechange, load e timers zero existentes. Resource interrompe o restante com rollback.

Não altera IPC, broker, sandbox ou capacidades do renderer. Não implementa parser bloqueante, async real, carregamento incremental ou sucesso universal de recursos no load. Promises e checkpoints não são novos. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem testes/fixtures executados. Itens 1, 2, 3 e 4 ABERTOS/parciais; item 19 adiado. Veja [contrato e limites](../releases/2026-11-08-document-lifecycle.md).

## Eventos programáticos: primeiro recorte do item 5

A VM oferece construção host opt-in e os bindings retêm Event/CustomEvent com identidade e detail rastreado pelo GC. DispatchEvent usa o runtime ativo e o caminho real do DOM, inclusive para nós destacados, sem simular ligação com Document. Listeners executam sincronamente e o dispatch JavaScript não cria checkpoints extras entre callbacks. Resource propaga até a transação exterior; erros ordinários são diagnosticados com continuação.

O cancelamento permanece interno ao evento e não aciona nem cancela ações padrão da interface/broker. Nenhuma nova autoridade de rede, I/O, IPC ou permissão é concedida. EventTarget independente, protótipos DOM, eventos especializados, Shadow DOM e entrada/edição completas ficam fora do recorte. Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional; item 5 ABERTO. Veja [contrato e limites](../specifications/events-subset.md).

## Especificidade CSS: primeiro recorte do item 6

web-css compara tuplas lexicográficas de IDs, classes/atributos/pseudoclasses e tipos, contabilizando ocorrências repetidas. :is/:not usam o máximo da lista e :where zero; somas verificadas recusam overflow. A prioridade distingue !important, inline, especificidade e ordem da regra/declaração. O matcher exige todos os IDs do composto; o índice conserva o primeiro ID como filtro necessário, sem substituir a verificação completa ou ampliar o orçamento.

Nenhuma dependência, API JavaScript, I/O, IPC ou permissão nova. CSSOM, custom properties completas, CSS-wide keywords, camadas/origens completas e conformidade geral permanecem abertos; @media/@supports já existiam. Implementação parcial; revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional; item 6 ABERTO. Veja [contrato e limites](../releases/2026-11-08-css-specificity.md).

## Alinhamento transversal Flex: primeiro recorte do item 7

web-layout usa altura de conteúdo definida para a linha row/row-reverse nowrap. Center/flex-end deslocam a caixa externa com margens, inclusive com valores negativos para overflow. Stretch só cresce visualmente caixas automáticas; não encolhe, recalcula percentuais ou estica controles diretos. O pipeline existente desloca pintura, clips próprios, métricas, links e controles e aplica recortes finais; clips ancestrais permanecem fixos. Sem nova API, cache, dependência, IPC ou capacidade de rede/arquivo. Altura automática, wrap, coluna e Grid conservam o caminho anterior. Veja [contrato e limites](../releases/2026-11-08-flex-cross-axis.md). Implementação parcial; revisão textual final do item 7 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–7 ABERTOS/parciais; item 19 adiado.

## Texto e whitespace: primeiro recorte do item 8

web-css separa os três aspectos de white-space; web-layout compartilha classificação linear entre fluxo e medição preferida por nó. Espaço colapsável pendente atravessa nós inline no fluxo, mas não transforma a agregação escalar de irmãos em dimensionamento intrínseco completo. Scans e cópias usam orçamento antecipado. Medição raw de controles e kerning preexistente são preservados. Sem nova dependência, fonte, fallback, pintor, IPC ou acesso a arquivos/rede. Shaping/bidi e tipografia completa continuam abertos. Veja [contrato e limites](../releases/2026-11-08-text-whitespace.md). Implementação parcial; revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–8 ABERTOS/parciais; item 19 adiado.

## Content-Type HTTP: primeiro recorte do item 9

O broker interpreta MIME e charset com parser puro limitado, integrado às respostas HTTP reais. A comparação da essência textual é exata e insensível a caixa ASCII, em vez de aceitar prefixos. Cabeçalhos ausentes, inválidos ou incompatíveis recusam o documento e omitem subrecursos pelo fluxo existente. O motor continua recebendo somente dados autorizados.

Permanecem ensure_verified/AppContainer, DNS público pinado, TLS padrão, portas 80/443, redirects revalidados, bloqueio de conteúdo misto, política de origem dos módulos e budgets de bytes/recursos/timeout. O ADR 0008 continua permitindo recursos públicos de CDN para CSS, imagens e scripts clássicos; este recorte não reverte essa política para a restrição histórica do ADR 0004. Não acrescenta I/O em web-*, IPC, permissões ou destinos.

Veja [contrato e limites](../releases/2026-11-08-http-content-type.md). Implementação parcial; revisão textual concluída sem bloqueadores e checagem estática aprovada. Sem testes ou validação funcional. Itens 1-9 ABERTOS/parciais; item 19 adiado.
## Histórico nativo: primeiro recorte do item 10

O [primeiro recorte do item 10, rolagem no histórico nativo](../releases/2026-11-08-history-scroll.md), conserva URL e offset CSS bidimensional por entrada, até 200 por aba, somente em memória. Push/Refresh começam no topo; Traverse aceito restaura uma vez com clamp na página exibida. Não implementa BFCache, restauração de formulários/JS, History API JavaScript ou persistência após reinício. Implementação parcial; revisão textual final do item 10 concluída sem bloqueadores; checagem estática aprovada, sem validação funcional. Itens 1–10 ABERTOS/parciais; item 19 adiado.

A shell captura o offset efetivo de ScrollArea::show dividido pelo zoom, sem fator DPI adicional. Coordenadas finitas e não negativas pertencem à entrada da aba. A restauração pendente só é consumida ao exibir a página correta; HOME/SETTINGS não capturam offsets de documento. Reflow/Event/Input/Change/Tick retêm a posição; falhas e respostas obsoletas não comprometem histórico ou restauração de outro destino. Documento e VM continuam sendo recarregados na travessia; nenhuma nova capacidade de I/O, IPC ou permissão é concedida.

## sessionStorage: primeiro recorte do item 11

A shell possui TabSessionStorage em memória por aba, independente da vida do renderer. O supervisor deriva a origem canônica HTTP(S) da URL final do broker e entrega só o seed corrente antes de run_initial. O filho devolve snapshot ordenado; versão, ID, página inteira, acesso e entradas precisam ser válidos antes da confirmação. Origem e identidade vêm do supervisor, nunca da resposta do renderer.

O IPC v5 introduziu SessionStorageSeed/SessionStorageSnapshot e validação nos dois extremos. Há 128 pares, chave 1 KiB, valor 16 KiB e soma 64 KiB/origem; 16 origens/aba, sem eviction. Origem nova vazia não ocupa slot; bucket existente limpo permanece. JSON é desserializado antes das quotas semânticas: frame de 48 MiB não elimina alocações transitórias nem substitui os limites menores.

Refresh, histórico, A→B→A e HOME/SETTINGS preservam buckets da aba; fechar/reiniciar elimina. Navegação inicial superada descarta escritas. Operação em andamento de sessão comprometida pode guardar checkpoint antigo validado, sem publicar página antiga, antes da próxima navegação. Crash/timeout/frame inválido conserva último checkpoint confirmado. Resource restaura storage junto com DOM/VM; quota ordinária é capturável e atômica.

file/noir/data/origem opaca/ausente são recusados, sem bucket null. Sem serialização no perfil, logs ou IA, I/O nos crates web-*, nova autoridade, localStorage, cookies, workers ou offline. Veja [ADR 0017](../adr/0017-bounded-session-storage.md) e [subconjunto](../specifications/session-storage-subset.md). Item 11 parcial; revisão textual final concluída sem bloqueadores; checagem estática aprovada, sem validação funcional.

## Dimensões naturais de imagens: primeiro recorte do item 12

ImageAsset conserva width/height inteiros para a textura RGBA e transporta natural_width/natural_height f32 obrigatórios em CSS px de densidade 1. A versão do protocolo passa de 5 para 6 por incompatibilidade de dados, sem novo canal, autoridade, dependência ou acesso a arquivos/rede. O renderer recebe somente recursos autorizados pelo broker. Metadados finitos, positivos, até 4096 por lado e ceil(width) × ceil(height) até 4 Mi pixels são validados separadamente dos bytes e nunca determinam alocação RGBA. Textura, uploads, sampling e rasterização continuam usando as dimensões físicas limitadas.

Veja [contrato de imagens](../specifications/images-subset.md) e [release do item 12](../releases/2026-11-08-natural-image-dimensions.md). Implementação parcial; revisão estática final do item 12 concluída sem bloqueadores; checagem estática aprovada. Sem validação funcional ou visual. Itens 1-12 ABERTOS/parciais; item 19 adiado.

## Metadados WAV locais: primeiro recorte do item 13

O broker lê uma nova classe de arquivo local dentro da raiz já autorizada, sem ampliar sandbox, permissões ou rede. Somente audio original ativo com src relativo .wav e preload=metadata explícito autoriza coleta; o IPC v7 leva metadados validados, não PCM ou handles. Limites: oito tentativas incluindo falhas, 4 MiB/arquivo aceito, 128 chunks e 8 MiB efetivamente lidos incluindo inválidos/sentinelas, sem reembolso. EOF deve ser observado antes de esgotar o agregado; não há leitura adicional nem garantia de snapshot atômico. A TOCTOU canonicalização/open preexistente permanece.

O seed único antes dos scripts vincula metadados aos nós originais. duration é somente leitura, real ou NaN; clones/nós novos não herdam dados. src alterado/removido invalida até nova sessão, inclusive valor igual/restaurado. preload aplica gate corrente reversível sem recarga. State integra mapa/DOM no rollback Resource; erro ordinário preserva efeitos. Não há playback, estados/eventos de mídia, clock/seek, vídeo, remoto, streaming ou dispositivos. Checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores, sem validação funcional. Veja [ADR 0018](../adr/0018-brokered-local-audio-metadata.md) e [contrato](../specifications/audio-metadata-subset.md).

## Observação textual limitada: primeiro recorte do item 17

A shell extrai um snapshot limitado de texto/display list, controles e links já confirmados, sem introduzir acesso geral a DOM, acessibilidade ou imagem. A cópia é incremental e a enumeração de abas é limitada antes de clonar títulos/URLs. ai-context prepara a observação por referência, aplicando permissões antes de copiar ou serializar campos. ai-runtime só utiliza JSON completo produzido dentro do orçamento; falha retorna erro estruturado, sem enviar um prefixo inválido ao modelo.

Identidade da aba e geração continuam vinculando proposta e observação. Alterações de items, links ou title já atualizam a geração; este recorte não corrige nem altera timers. Não muda grants, full_access, vocabulário de ferramentas, limites de ações ou broker. Cancelar impede continuação, mas não desfaz operação já iniciada. Campos password/hidden são excluídos; texto visível, URLs e campos comuns ainda podem conter segredos. Dados de página não concedem autoridade, e a validação não prova imunidade a prompt injection.

Veja [limites e estado da entrega](../releases/2026-11-08-bounded-ai-observation.md) e o complemento ao [ADR 0006](../adr/0006-ai-browser-tools.md). Item 17 parcial, checagem estática aprovada; revisão estática final concluída sem bloqueadores; sem testes ou validação funcional.

## Segundo ciclo: WebAssembly i64 e BigInt

O [segundo recorte do item 2](../releases/2026-11-08-webassembly-i64-bigint.md) acrescenta i64 ↔ BigInt exato nas funções exportadas stateless, sem passagem por f64. Argumentos usam ToBigInt (Number rejeitado, hint number em objetos, strings/booleanos e redução módulo 2^64); o retorno é BigInt signed. Store continua efêmero por chamada, com preflight, quotas, fuel e rollback preservados. Imports, memória/tabelas, start, globals mutáveis, segmentos e referências continuam recusados. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Não é WebAssembly completo. Itens 1-18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
## Segundo ciclo: seletores DOM/CSS compartilhados

O [segundo recorte do item 3](../releases/2026-11-08-shared-dom-selectors.md) substitui o parser simples de querySelector/querySelectorAll/matches/closest pelo parser e matcher CSS próprios. Acrescenta compostos, listas estritas, combinadores, atributos, escapes e os seletores lógicos/estruturais do subconjunto CSS existente. Mantém ordem da árvore, deduplicação, snapshot estático, closest destacado e fronteiras template/noscript. O matching pode consultar ancestrais fora da raiz; :scope, :has, namespaces, pseudoelementos, coleções vivas e Shadow DOM continuam pendentes. Limites locais e orçamento DOM compartilhado permanecem obrigatórios, inclusive em erros. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem testes/fixtures executados ou validação funcional. Itens 1–18 ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Evidências anteriores permanecem históricas.
