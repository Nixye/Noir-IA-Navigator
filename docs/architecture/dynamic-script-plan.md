# Plano: scripts clássicos dinâmicos autorizados pelo broker

Estado: proposta de arquitetura, não implementada. Revisão estática em 2026-11-09.
Nenhuma fonte de produção, executável ou distribuição foi alterada por esta investigação.
Não foram executados Cargo, rede, GUI ou JavaScript capturado. O snapshot foi lido
somente como dados. Os contratos abaixo são propostos, não APIs já existentes.

## 1. Resultado útil e recorte recomendado

O menor incremento funcional não é aceitar `script.src` como expando. É ligar
uma preparação de script no DOM a um pedido limitado, validar esse pedido no
supervisor, buscar a fonte no broker e devolver os bytes à **mesma VM retida**.
Não executar código no broker, no shell, em Node/PowerShell nem em uma VM local
sem sandbox. Não entregar rede ou arquivos aos crates `web-*`.

Primeiro recorte recomendado:

- Scripts clássicos externos criados por `document.createElement('script')`,
  preparados depois de `document.readyState === 'complete'`.
- Conectar um script com `src`, ou definir `src` em um script já conectado.
  `appendChild`, `insertBefore` e `replaceChild` precisam convergir para o mesmo
  hook de preparação, inclusive ao conectar uma árvore contendo o script.
- Async implícito de scripts criados por script e `async=true`. `async=false`
  é recusado explicitamente até existir a fila ordenada correspondente.
- HTTPS, mesma origem do documento final, sem credenciais, sem CORS, sem SRI.
  Redirecionamentos permanecem dentro dessa origem e nunca reduzem HTTPS.
- Apenas documentos carregados com contexto de autorização vivo do broker.
  Snapshot, `file:`, `noir:`, renderer descartável e contextos sem metadados
  suficientes não ganham busca dinâmica.
- Política CSP desconhecida ou CSP obrigatória presente e não implementada
  bloqueia novas buscas dinâmicas. A primeira entrega não interpreta nonce como
  permissão e não presume ausência de CSP a partir do HTML.

Este recorte cobre a **forma** do loader que falha no timer do Google capturado.
Não demonstra que os recursos reais cabem nas cotas, que a política HTTP os
permite, nem que a VM execute toda a fonte recebida. Não declarar Google funcional
somente porque desapareceu `Unsupported DOM assignment src`.

## 2. Evidência reproduzível do caso capturado

Entradas locais de diagnóstico:

- `target/validation/google-diagnostics-cycle16/google-home.noir-snapshot`
- `target/validation/google-diagnostics-cycle16/google-home-replay.json`

A captura contém HTML com 86.824 bytes, 11 scripts inline, zero `ScriptAsset`,
duas imagens e nenhum metadado de áudio. Origem final: `https://www.google.com`.
Não há `<base>` nem meta CSP no HTML. Os cabeçalhos HTTP **não** são preservados
pelo formato v1, portanto a CSP da resposta é desconhecida neste artefato.

O replay registra o erro no timer, receptor DOM node=112, tag=script, valor string.
O script inline 11 agenda um timer de zero milissegundos que chega à sequência
equivalente abaixo. Os nomes/URL voláteis foram omitidos; não são uma fixture
extraída para execução:

1. `document.createElement('script')`.
2. Registro de `onerror` e `onload` no elemento.
3. Atribuição de um `src` relativo em `/xjs/_/js/...`.
4. Leitura de `script.src` para rastrear o recurso em uma lista da página.
5. `document.body.appendChild(script)`.

Não há type=module, crossorigin, integrity nem atribuição de async nesse caminho.
O async verdadeiro é implícito, como em um script clássico criado por script.
O loader também consulta `document.currentScript` em um caminho condicional;
essa propriedade não deve ser fabricada como outro script ou deixada permanentemente
apontando para o último executado.

Outros caminhos observados, distintos do timer que produziu o diagnóstico:

- Script inline 5: clique cria script, lê URL de `data-script-url` e insere no body.
- Script inline 9: loader de barra cria SCRIPT, atribui async=true,
  type=text/javascript, charset=UTF-8, src e nonce, registra onerror e insere no
  HEAD. Também tenta criar stylesheet dinâmico, que fica fora deste incremento.
- Script inline 11: cria links rel=preload; não interpretar isso como autorização
  para baixar ou executar JavaScript adicional nesta entrega.

Não copiar para o ADR tokens, nonce, query strings ou configuração embutida da
página. A leitura textual não mediu tamanho, MIME, redirects ou CSP dos recursos
que ainda não foram baixados.

## 3. Fluxo atual e lacunas concretas

### Broker

`crates/browser-core/src/network.rs::load` baixa HTML, faz parsing sem execução,
descobre recursos estáticos e devolve `DocumentInput`. `module_assets::Scripts`
compartilha quatro tentativas entre clássicos e módulos, 128 KiB por fonte e
256 KiB entregues. As tentativas fracassadas não aparecem em `DocumentInput`.

`network::fetch_with_policy` já aplica portas 80/443, DNS público validado e
fixado, sem proxy, sem Referer, sem cookies, redirects limitados, proibição de
downgrade e prazo. Módulos usam mesma origem; clássicos estáticos podem usar CDN
pública pelo ADR 0008. Não reutilizar a permissão mais ampla de clássicos estáticos
para o novo canal dinâmico por acidente.

`HttpMetadata` guarda apenas Content-Type/charset. Não há atualmente contexto
persistente do broker, orçamento vivo por documento nem estado CSP retido.

### Bindings e renderer

`web-bindings::DocumentSession` conserva documento, VM e State. `execute` preserva
efeitos anteriores a erros comuns e reverte operações Resource, desativando scripts.
Os hooks de inserção e `set_attribute` só alteram DOM. `src` de script não tem
reflexão funcional; `setAttribute('src', ...)` sozinho também não busca nada.

`lifecycle.rs::run_initial` fotografa IDs dos scripts iniciais, executa o subconjunto,
faz DOMContentLoaded/load e roda timers zero. Uma varredura indiscriminada posterior
de todos os scripts reexecutaria conteúdo inicial e poderia ativar scripts inertes
de innerHTML. Não usar essa estratégia.

`web-renderer::RenderSession` já possui a sessão retida e caminhos separados para
evento, timer e animation frame. Deve apenas encaminhar os novos dados e executar
a conclusão dentro dessa mesma sessão.

### Supervisor e desktop

IPC v8 tem Begin/Reflow/Event/Tick/AnimationFrame. `RenderResponse` só contém
identidade, página/erro e storage. Não há pedidos de recurso nem conclusão de script.
`browser-process::RendererSession::exchange` valida a resposta inteira antes do
checkpoint e tem timeout de 10 s; rede não pode ser embutida nessa espera de IPC.

`apps/browser-desktop/src/shell.rs` já faz operações em thread host, conserva
`Arc<Mutex<RendererSession>>`, descarta resultados se outra navegação está enfileirada
e verifica identidade de sessão/documento para storage. Porém `generation` muda
também em interações, e `committed_document` só muda no commit: nenhum dos dois,
isoladamente, serve como token de cancelamento de uma carga nova ainda pendente.

## 4. Decisão ADR proposta

Registrar um novo ADR quando começar a implementação, com número disponível na
ocasião. Fronteira nova: um renderer não confiável pode **propor** um `src` ao broker.
A proposta não é uma capability de rede, não escolhe origem/base/política e não
pode gerar operações privilegiadas genéricas.

O contexto de autorização é criado pelo carregamento real do broker e fica fora
do IPC e fora dos campos que o renderer pode controlar. Não adicionar ao pedido
campos como `document_url`, `origin`, `csp_absent`, cookies, headers arbitrários,
root de filesystem, URL final de confiança ou permissão de redirect.

Não mudar a sandbox do Windows, capabilities AppContainer, firewall, credenciais,
certificados ou configurações do usuário. Implementar este protocolo em código
não exige relaxar essas configurações. Se uma implementação depender de relaxá-las,
isso é outro escopo: parar e pedir a autorização específica, sem fallback.

O ADR deve declarar que a superfície passa de descoberta estática para pedidos
originados por execução de página. A análise de SSRF, exfiltração para a própria
origem, loop de onerror, respostas antigas e renderer comprometido é obrigatória.

## 5. Contratos de interfaces

### 5.1 API host do broker, fora de browser-ipc

Nomes indicativos:

```rust
pub struct LoadedDocumentWithContext {
    pub document: DocumentInput,
    pub resources: DocumentResourceContext,
}

pub fn load_with_context(input: &str)
    -> Result<LoadedDocumentWithContext, String>;

impl DocumentResourceContext {
    pub fn fetch_classic(&mut self, request: &ClassicScriptRequest)
        -> ClassicScriptOutcome;
    pub fn cancel(&self);
}
```

`DocumentResourceContext` tem campos privados e não é desserializável. Construtores
com autoridade só existem no loader. Retém URL final original, decisão CSP,
estado de cancelamento e ledger de tentativas/bytes/tempo. Pode ser um handle
compartilhado, mas todos os clones devem compartilhar o mesmo ledger; clonar não
renova orçamento. A identidade de aba/sessão fica no shell e vincula esse handle
a uma única carga/documento. Não oferecer `from_document_input` ou um construtor
público que aceite `CspAbsent` vindo de um chamador genérico.

`load_document` permanece compatível como wrapper que descarta o contexto. CLI
capture/replay não reconstrói autorização a partir de URL contida no snapshot.
Os dados iniciais continuam no IPC atual, sem levar o contexto privilegiado junto.

Alterar `module_assets::Scripts` para exportar ao loader um resumo privado de
orçamento ou transferir seu ledger. Não estimar tentativas por `scripts.len()`:
isso perderia falhas de rede e permitiria renovar a cota depois de quatro falhas.

Política da primeira versão:

- Resolver `src` com a URL final guardada pelo broker. Sem suporte a `<base>`
  nesta via; documento com base que altere resolução deve ser bloqueado/diagnosticado
  até existir base autorizada independente do renderer.
- Validar comprimento de 8192 bytes antes de parsing, controles, credenciais,
  esquema, porta, mesma origem HTTPS e endereço público. Remover fragmento somente
  para a requisição HTTP, conservando identidade lógica bem definida.
- Validar cada redirect antes de sua requisição, incluindo mesma origem. Não
  resolver CORS por aceitar `crossorigin` fornecido pela página.
- Reusar o cliente restrito e decodificador; exigir MIME JavaScript reconhecido
  e verificar tamanho bruto e tamanho UTF-8 decodificado separadamente.
- Cancelamento/prazo são verificados antes de DNS, request, redirect, leitura e
  entrega. Uma operação de DNS/HTTP bloqueante já iniciada pode só parar no timeout
  atual; documentar isso em vez de prometer cancelamento instantâneo.

### 5.2 CSP e metadados obrigatórios

No carregamento inicial, guardar presença de Content-Security-Policy obrigatório
dos cabeçalhos reais, com contagem/tamanho limitados, e verificar metas relevantes
no HTML original. Não usar apenas `HttpMetadata::from_fields(Content-Type)` para
concluir sobre CSP. Ausência conhecida e política desconhecida são estados diferentes.

Primeira versão aceita somente ausência conhecida. CSP presente, inválida,
excessiva, desconhecida ou não suportada produz recusa explícita. Report-Only
deve ser distinguida de política obrigatória; não é uma autorização adicional.
Os nonces observados no HTML não provam a política nem autorizam strict-dynamic.

Inserção/mutação dinâmica de meta CSP não pode silenciosamente manter novas buscas
liberadas. Implementar restrição monotônica/fail-closed no novo mecanismo e testes:
uma sinalização vinda do renderer pode apenas revogar/restringir autorização,
nunca declarar ausência, relaxar ou trocar a política do broker. Se essa revogação
ainda não estiver disponível, diagnosticar e desabilitar a via dinâmica ao encontrar
esse caso. Não declarar CSP completa. Isto pode continuar bloqueando Google até
ser conhecido e implementado seu contrato real de política.

### 5.3 Tipos IPC propostos

```rust
pub struct ClassicScriptRequest {
    pub request_id: u64,
    pub node: u64,
    pub src: String,
}

pub enum ClassicScriptOutcome {
    Loaded { final_url: String, source: String },
    Failed { reason: ClassicScriptFailure },
}

// Novo campo obrigatório de RenderResponse:
// classic_script_requests: Vec<ClassicScriptRequest>

// Nova variante de SessionRequest:
// CompleteClassicScript { id, request_id, node, outcome, viewport }
```

O nome `ClassicScriptRequest` não é prova de que o renderer observou um script
legítimo. Mesmo um renderer comprometido só alcança a política host já restrita.
O node é identidade de retorno, nunca índice usado pelo broker para autorizar acesso.
`final_url` é dado do broker no sentido host→renderer, nunca uma declaração aceita
no sentido inverso. `reason` deve ser enum limitado (Policy, Network, Mime, Size,
Budget, Cancelled, Unavailable), não corpo HTTP/erro arbitrário com dados privados.

Incrementar versão do protocolo; não adicionar defaults que escondam campo ausente.
A fila precisa de Deserialize limitado antes de crescer e validação de IDs,
duplicatas, strings e orçamento agregado. Outcome tem no máximo uma fonte por
mensagem. `id` da operação IPC e `request_id` do recurso são domínios distintos.

O supervisor conserva requests pendentes validados e uma política host de sessão
habilitada/desabilitada. Conclusão desconhecida, repetida, com node diferente ou de
outra sessão é rejeitada antes da VM. O renderer faz as mesmas validações para
seus registros pendentes. IDs são monotônicos com overflow fechado.

A sessão de renderer aceita Begin uma vez, já como hoje. Isso vincula os IDs IPC
ao documento sem permitir ao renderer trocar a origem. Um token host adicional
vincula aba/carga/sessão fora do IPC, inclusive reload da mesma URL.

Validar identidade, página, storage **e pedidos** antes de atualizar qualquer
checkpoint ou expor pedidos ao broker. Resposta malformada não pode gerar I/O,
nem atualizar storage parcialmente. `take_classic_script_requests()` só expõe
o lote que passou por essa validação completa.

### 5.4 Estados e execução nos bindings

Criar módulo separado `web-bindings/src/scripts.rs`, sem I/O. Manter proveniência
por NodeId e os estados New, Prepared, Requested, Finished/Failed. O DOM retido
não recicla seus IDs atualmente; ainda assim não confiar só em tag/src no retorno.

- Scripts iniciais, scripts criados pelo parser de innerHTML e nós inertes não
  são promovidos por varredura. `cloneNode` deve preservar estado already-started
  relevante ou ser explicitamente excluído do subconjunto nesta primeira entrega.
- A preparação captura `src` e atributos antes de emitir o pedido. Elemento
  desconectado não busca. Conexão só prepara uma vez, após as verificações DOM
  e orçamento; conectar subtree deve visitar somente seus candidatos limitados.
- `src` retorna URL resolvida com a base do documento, sem alterar a string do
  atributo. `getAttribute('src')` conserva texto original. Setter refletido e
  setAttribute compartilham o mesmo hook. Implementar has/get/set coerentes,
  não expando que promete uma plataforma inexistente.
- Implementar explicitamente type e async necessários ao recorte. Charset vazio
  ou UTF-8 pode ser aceito no subconjunto, mas charset declarado não substitui a
  decodificação HTTP validada. Outros valores não devem simular suporte.
- type=module, dynamic inline, async=false, integrity e crossorigin ficam fora
  do primeiro recorte. Produzir erro/diagnóstico terminal por elemento; não load.
- Ao preparar depois de Complete, registrar pedido em State transacional. Não
  fazer I/O dentro do setter/appendChild. Erro comum posterior preserva o pedido,
  conforme ADR 0007; erro Resource reverte pedidos criados na operação e desativa
  scripts antes que qualquer pedido daquela operação seja exportado.
- Depois de preparado, mudar/remover src, desconectar/reconectar ou mover o nó
  não retargeta nem executa novamente. Remoção de um script após início não é
  tratada como cancelamento mágico de fetch; cancelamento obrigatório é de sessão.
- Antes de executar uma conclusão, consumir o pedido exatamente uma vez e debitar
  orçamento fora do rollback que poderia reabrir sua execução. Chamar o mesmo
  `DocumentSession::execute`, sem recriar runtime ou chamar `run_initial`.
- Durante avaliação clássica, currentScript aponta para aquele elemento e é
  restaurado em todos os caminhos de erro. O checkpoint de microtasks não deve
  deixar currentScript artificialmente ativo; verificar a API atual do runtime
  e separar escopo de avaliação/checkpoint se necessário. Não inventar valor.
- Sucesso de obtenção/execução despacha load real do elemento; falha de fetch,
  política ou MIME despacha error. Exceção comum de JavaScript vai para diagnóstico
  e window.onerror, não é convertida em falha de rede. Definir e testar a distinção
  de parse/runtime error e load segundo o subconjunto antes de marcar concluído.
- O evento deve atingir também um elemento removido depois de preparado. O atual
  `dispatch_event` exige conexão: acrescentar um caminho interno de evento de
  recurso que não finja conexão e preserve target/this, não-bubbling e once.
- Source e callbacks podem enfileirar outro script, processado somente no próximo
  lote host. Não recursar a busca a partir da VM nem promover módulos registrados.
- Resource fatal cancela pendentes e impede load fictício. Um timer/evento futuro
  não pode reiniciar scripts de uma sessão desativada.

Scripts dinâmicos anteriores a Complete continuam explicitamente fora do recorte.
Suportá-los corretamente exigirá pausar/concluir lifecycle inicial e atraso de load;
não basta baixar depois de um load que já foi anunciado. O timer zero observado
no Google é executado depois da transição Complete pelo fluxo atual e não exige
essa ampliação para provar a primeira vertical.

## 6. Pump host, cancelamento e publicação transacional

O desktop coordena browser-core e browser-process; não adicionar dependência de
browser-core a web-renderer/web-bindings. Novo helper host em
`apps/browser-desktop/src/script_resources.rs` evita ampliar ainda mais shell.rs.

Fluxo mínimo, serial e limitado:

1. Carregar `load_with_context`; criar token de carga e renderer com sandbox verificada.
2. Executar Begin ou operação retida normal e validar sua resposta.
3. Obter um lote limitado de pedidos do supervisor. Associá-los ao contexto da
   mesma carga, não à URL atualmente escrita na barra de endereço.
4. Soltar Mutex<RendererSession> antes do fetch. Fazer I/O somente no worker host,
   nunca no thread egui nem dentro do timeout de uma troca IPC.
5. Conferir cancelamento/identidade antes e depois do fetch; entregar uma conclusão
   ao mesmo Arc de sessão. Conferir novamente ao publicar página/checkpoint.
6. Repetir somente dentro das cotas totais. Interromper imediatamente entre tarefas
   quando uma navegação/fechamento tornou o token obsoleto.

Uma fila serial de async é uma implementação conservadora possível, sem garantia
de concorrência ou ordem de chegada equivalente aos navegadores completos. Não
suporta async=false por acidente. Declarar que entradas/timers/frames podem aguardar
o lote host; uma melhoria posterior pode intercalar operações com um scheduler.

Criar token monotônico de **carga/documento**, com flag de cancelamento partilhada,
antes da carga inicial. Cancelar ao enfileirar navegação que substitui documento,
reload, fechar aba/janela ou encerrar sessão, e não apenas quando houver commit.
Reflow, timer, frame e input não substituem esse token. Não usar saturating counter
que possa aceitar IDs repetidos no overflow.

O resultado só atualiza a aba se token atual, documento comprometido, Arc de sessão
e estado sem navegação substituta corresponderem. Preservar as regras existentes
de checkpoint de storage; não transplantar storage ou página entre origens porque
duas sessões emitiram request_id=1. Uma carga antiga pode acabar o request bloqueante
já enviado, mas não iniciar redirects novos nem publicar/executar o retorno depois
de cancelada. Se conclusão já entrou no renderer antes do cancelamento, não prometer
rollback retroativo; descartar publicação e destruir a sessão ao encerrar.

Para a primeira vertical, o pump pode estar no worker de navegação já existente.
Não segurar a UI por espera síncrona e não aumentar o timeout de IPC de 10 s para
esconder os até 25 s de rede. Um segundo canal de completion pode vir depois; não
é necessário abrir um servidor de rede no renderer.

## 7. Cotas propostas, sem aumento dos limites atuais

- Até quatro tentativas de rede de scripts por documento, compartilhadas com
  clássicos/módulos estáticos, inclusive falhas e repetição de URL em outro nó.
- Até quatro preparações dinâmicas exportáveis por sessão e quatro pedidos por
  lote. In-flight de rede um no primeiro pump. Quinto pedido não inicia I/O.
- src e URL final: até 8192 bytes cada, sem controles/credenciais; fila no máximo
  32 KiB de src, além de overhead fixo. Erros usam enum limitado.
- 128 KiB bruto e UTF-8 decodificado por fonte; 256 KiB de fontes externas entregues
  por documento, compartilhados com entrega estática, não por resposta IPC.
- Reusar um limite agregado de execução de fontes de 512 KiB para inicial mais
  dinâmica. O contador de lifecycle hoje é local: torná-lo de sessão para que
  sucessivos callbacks não renovem o orçamento.
- Deadline inicial de rede permanece 25 s. Para recursos posteriores, manter
  orçamento host cumulativo de tempo ativo de rede restante, não renovar 25 s a
  cada timer. Não usar simplesmente `Instant` inicial para proibir qualquer recurso
  após 25 s de ociosidade: distinguir tempo ocioso de tempo consumido em I/O.
- Manter timeout por conexão/request/DNS e limite de cinco redirects. Cancelamento
  pode retornar antes do timeout apenas onde o transporte permite.
- Cotas e contadores de I/O são monotônicos host; erro, rollback JS, reflow, clone
  de handle, redirect e retry não os restauram. Ledger de renderer não substitui
  ledger do broker.

Um recurso do Google acima de 128 KiB deve falhar com diagnóstico de tamanho.
Não truncar código e executar prefixo, não aumentar limite sem revisão e medição.

## 8. Separação de trabalho por arquivo

### A. Contrato IPC e supervisor

- `crates/browser-ipc/src/lib.rs`: tipos, enum de falhas, campo obrigatório de
  pedidos, variante de conclusão, constantes/validadores e bump de protocolo.
- `crates/browser-ipc/src/bounded.rs`: reusar/adicionar coleções limitadas.
- `crates/browser-process/src/session.rs`: pending ledger, validação transacional,
  métodos take/complete, roteamento no worker, rejeição de replay/fora de sessão.
- `crates/browser-process/src/lib.rs`: adaptar renderer descartável explicitamente
  sem serviço dinâmico e construir respostas completas.
- Atualizar todos os construtores de RenderResponse/RenderRequest e testes de
  protocolo. Congelar estes contratos antes de trabalhos B/C/D em paralelo.

### B. Broker e contexto de autorização

- `crates/browser-core/src/lib.rs`: `load_with_context`, wrapper compatível e
  exportação de tipos host sem construtor público de autoridade.
- Novo `crates/browser-core/src/script_resources.rs`: contexto, ledger, revogação,
  política, testes puros com transporte injetável interno.
- `crates/browser-core/src/network.rs`: captura de metadados CSP, reuso do fetch
  restrito com política same-origin e cancellation/deadline explícitos.
- `crates/browser-core/src/http_metadata.rs`: apenas ajustes pertinentes à captura
  limitada; manter Content-Type separado de decisões de autorização se isso evitar
  confundir ausência de header com parse de MIME.
- `crates/browser-core/src/module_assets.rs`: transferir ledger de tentativas/bytes
  estáticos sem expor credenciais ou dar descoberta ao renderer.

### C. Bindings e integração de execução

- Novo `crates/web-bindings/src/scripts.rs` e `scripts_tests.rs`: estados,
  preparação, conclusão, reflection, eventos e orçamentos.
- `crates/web-bindings/src/lib.rs`: hooks mínimos de criação/inserção/atributos,
  has/get/set, rollback e disable_scripts. Coordenar explicitamente esse arquivo
  compartilhado com o trabalho de descritores; evitar edições concorrentes na
  mesma região e não expandir exóticos de descritores para fingir suporte de DOM.
- `crates/web-bindings/src/lifecycle.rs`: marcar proveniência inicial, contador de
  fonte de sessão e recusa explícita de pré-Complete; preservar ordem de lifecycle.
- `crates/web-bindings/src/events.rs`/`diagnostics.rs`: recurso interno e origem
  de diagnóstico sanitizada quando necessário, sem mascarar falha como sucesso.
- `crates/web-renderer/src/lib.rs`: encaminhar pedidos/conclusões e warnings,
  conservar a VM e integrar resultado à página.

### D. Shell e validação da vertical

- Novo `apps/browser-desktop/src/script_resources.rs`: pump host limitado.
- `apps/browser-desktop/src/main.rs`: declaração do módulo, se necessária.
- `apps/browser-desktop/src/shell.rs`: contexto/epoch/cancel por carga, wiring do
  worker, resultados retidos e rejeição de stale; compartilhar edits com cuidado.
- `apps/browser-cli/src/snapshot.rs` e testes: nenhuma autorização viva no snapshot.
  O protocolo novo torna snapshots antigos incompatíveis pelo contrato atual;
  registrar isso, não reescrever o header do arquivo antigo para burlar validação.
  Reproduzir cycle16 com o binário correspondente preservado; capturar novo caso
  pelo fluxo autorizado quando o novo build estiver validado.
- ADR novo, capabilities.md, html-dom-subset.md, javascript-subset.md,
  script-diagnostics.md e fixture sintética não remota, somente após comportamento
  implementado e testes. Não marcar a proposta como funcional na matriz.

## 9. Testes de aceitação e fronteiras

Não fazer testes negativos de SSRF acessando serviços privados reais. Injetar um
transporte/resolvedor fake apenas nos testes do broker e conferir **zero chamadas**
para entradas recusadas. A implementação de produção continua usando a rede real
restrita; não introduzir bypass de loopback via flag de produção.

### Bindings, sem rede

1. Script desconectado não pede. src antes/depois da conexão pede uma vez.
2. createElement + src + onload/onerror + appendChild, com fonte sintética autorizada,
   muda o DOM e um global já existente na mesma VM. Um timer anterior continua vivo.
3. Inserção por subtree, insertBefore e replaceChild usa a mesma preparação.
4. Alterar src/remover/mover após preparo não retargeta; reinserir não reexecuta.
5. innerHTML, template/inativo, scripts iniciais e clones already-started não ganham
   execução nova; clone de script New tem política explícita e testada.
6. async implícito/refletido, src absoluto versus atributo relativo, currentScript
   durante avaliação, depois dela e em microtask/callback.
7. Módulo, inline dinâmico, async=false, SRI, CORS e pré-Complete: erro verificável,
   sem fonte executada ou load falso.
8. Falha de obtenção gera error uma vez; sucesso gera load uma vez com target/this,
   inclusive se o nó foi removido; exceção de execução não vira NetworkFailure.
9. Erro ordinário após append preserva pedido; Resource reverte pedidos não exportados,
   cancela pendentes e não permite reiniciar quotas pelo callback onerror.
10. Execução que cria outro script não recursa para rede e não duplica requests.

### IPC/supervisor

1. Roundtrip da variante/campo obrigatório; protocolo antigo/campo ausente recusado.
2. Contagem N+1 recusada antes de desserializar pedido excedente, src acima do limite,
   IDs zero/duplicados/overflow, soma de bytes e fonte acima de 128 KiB.
3. Resposta com página/storage inválidos mas pedidos válidos não provoca I/O.
4. Conclusão antes de Begin, desconhecida, repetida, de outro node/sessão ou fora
   da cota não executa e não avança checkpoint de forma parcial.
5. Fonte chegada por completion não satisfaz módulo/import registrado por engano.

### Broker

1. URL same-origin HTTPS aceita pelo transporte fake; outra origem, HTTP, portas,
   credenciais, controles, javascript/data/blob/file/noir, localhost, private IPv4,
   IPv6, mapped IPv4 e DNS contendo endereço privado são negados.
2. Redirect fora da origem ou downgrade recusado antes de visitar o próximo alvo;
   DNS fixado e nenhuma herança de proxy/cookie/Referer.
3. Content-Type errado, repetido/ausente, corpo grande e expansão de charset são
   recusados antes da entrega à VM. Source nunca é truncada.
4. Quatro falhas estáticas deixam zero tentativas dinâmicas; clonagem de contexto,
   retry e repetição de src não renovam cotas. Strings/tentativas com falha consomem
   o trabalho apropriado, sem criar uma fila infinita de rejeições.
5. CSP HTTP presente/ausente conhecida/desconhecida, meta inicial e revogação dinâmica
   têm comportamento fechado e não podem ser relaxadas por dados de renderer.
6. Cancelar antes de fetch dá zero chamadas; durante redirect não visita próximo
   destino; depois do fetch não entrega bytes. Deadline acumulado não se reinicia.

### Retenção, desktop, sandbox e diagnóstico

1. Fixture sintética autoral, executada no caminho de sessão remota AppContainer
   quando a URL é HTTPS, usa bytes de teste autorizados pelo host. Nunca usar
   conteúdo remoto capturado disfarçado de fixture file/noir.
2. Primeiro provar fonte pequena que muda DOM, preserva estado e dispara load.
   Depois incluir erro, requests encadeados, input/timer/frame e limites.
3. Navegar A→B enquanto A baixa, recarregar mesma URL, fechar aba, trocar perfil,
   falhar renderer e repetir operação: A nunca executa/publica retorno em B.
4. Demonstração de que renderer ainda não acessa rede/arquivo/processos, pelo
   caminho de prova existente, sem alterar capabilities para fazer o teste passar.
5. Replay v1 continua sem buscar `/xjs/`; origem HTTPS e exigência de AppContainer
   permanecem. Snapshot antigo não é modificado para parecer da versão nova.
6. Só então teste Google vivo autorizado, com diagnóstico de policy/MIME/tamanho
   e primeira exceção real. Separar melhoria de loader de descritores, CSS,
   stylesheet dinâmico e APIs de página ainda ausentes. Não usar console vazio
   como prova de funcionalidade.

Antes de uma entrega, executar os testes focados e os três gates do AGENTS.md:
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
e `cargo test --workspace --locked`. Nesta investigação nenhum deles foi executado;
o coordenador mantém sua validação global separada. Preservar dist até autorização
e validação próprias da entrega.

## 10. Critério de decisão

Implementar A+B+C e teste de sessão antes de integrar D ao desktop. Se o contexto
de política/ledger ainda não existir, manter o serviço de busca desabilitado.
Reflexão de src isolada pode ser um teste intermediário, mas não um item concluído.

O primeiro marco completo é: um clássico externo pequeno e autorizado, inserido
pós-load, realmente obtido pelo broker e executado uma vez na VM retida, com
load/error reais, quotas compartilhadas e navegação concorrente segura. O marco
Google é posterior e depende de evidência nova, não de exceções suprimidas.
