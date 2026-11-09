# Item 4: ciclo de vida inicial e scripts defer

Nota histórica: este documento descreve o primeiro recorte e a evidência disponível naquele momento. O segundo recorte acrescenta [frames limitados](2026-11-08-animation-frames.md); a nova autorização de testes e seus resultados estão no [baseline](../validation/2026-11-08-functional-baseline.md). A ausência de rAF e o adiamento mencionados abaixo referem-se àquela entrega.


Estado: Implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem testes/fixtures executados. Itens 1, 2, 3 e 4 ABERTOS/parciais; item 19 adiado. Sem navegação, validação gráfica, build, reconstrução de dist/Noir, commit ou publicação nesta etapa.

## Contrato do recorte

- document.readyState expõe loading, interactive e complete, como estado da sessão.
- run_initial permite uma única tentativa, inclusive após falha: chamadas repetidas, reflow, eventos e ticks não repetem scripts ou eventos iniciais.
- Clássicos inline e externos ordinários executam em loading, na ordem DOM. O DOM inteiro já está construído: não há parser incremental nem execução realmente bloqueante do parser.
- Depois dos clássicos ordinários, muda para interactive e emite readystatechange no Document, sem bubbling/cancelamento. document.onreadystatechange também é oferecido.
- Uma fila única conserva a ordem DOM entre clássicos EXTERNOS defer e módulos sem async, inline ou externos. O grafo/registro autorizado e as recusas anteriores de módulos continuam aplicáveis.
- Após a fila vem DOMContentLoaded, com bubbling até Window; depois complete, outro readystatechange, load em Window e o lote inicial de timers zero já existente.
- Clássico inline ignora async/defer. Clássico externo async, inclusive async defer, e módulo async são diagnosticados e não executados. Não se substitui execução assíncrona por execução síncrona silenciosa.
- Promises, queueMicrotask e checkpoints são mecanismos anteriores: checkpoints após cada script, módulo e callback, não uma nova fila Web ou checkpoints artificiais por etapa.
- Erro ordinário mantém a política de diagnóstico e continuação. Resource permanece fatal, com rollback da operação e scripts desativados, sem executar o lifecycle restante. Na própria transição/readystatechange, o estado volta ao anterior. Se Resource ocorrer já em load ou timer posterior, complete permanece: a transição já ocorreu. Complete não declara uma inicialização sem falhas. O preparo de grafo reverte seu registro parcial também em erro ordinário, preservando consumo de orçamento.

## Fronteiras e lacunas

O DOM e os assets são o lote já autorizado pelo broker. O renderer não abre src, não busca novos recursos e não recebe novas capacidades. Load descreve a conclusão deste pipeline limitado, não sucesso universal de imagens, CSS, fontes, frames e todas as categorias de recursos Web.

Não há parser incremental, scripts dinâmicos executáveis, async real, top-level await, avaliação assíncrona de módulos, fetch/I/O, requestAnimationFrame, scheduler de tarefas Web completo, visibilidade/congelamento/retomada/descarte padronizados ou coordenação geral entre documentos/workers. O item 3 continua parcial; innerHTML não passa a executar scripts.

## Limites

O snapshot inicial contém até 64 IDs de scripts. O 65º serve para emitir diagnóstico; os excedentes não executam, sem converter esse truncamento em Resource. IDs removidos ou tornados inativos antes de sua visita são ignorados; tipo, atributos e fonte são lidos na visita. Novos IDs não entram nessa inicialização. Depois de enfileirado, um defer/módulo usa a fonte/URL capturada, mesmo se o nó for removido ou alterado.

O plano cobra 512 KiB agregados de fontes antes de copiar/concatenar, incluindo raízes e dependências copiadas. A cobrança é conservadora e pode cobrar novamente fontes compartilhadas entre grafos. Cada grafo admite até 64 registros. A fila defer não autoriza mais assets nem orçamento independente.

Permanecem os limites anteriores do broker (quatro tentativas/arquivos externos, 128 KiB por fonte, 256 KiB entregues) e da VM/DOM. Há 128 listeners, 128 timers pendentes e 256 eventos retidos; os novos eventos iniciais também consomem o orçamento. Quotas compartilhadas podem interromper antes dos tetos individuais.

## Fixtures-fonte e evidências

- [Ordem de execução](../../fixtures/document-lifecycle/index.html): quatro assets locais, clássicos inline/externos, defer, módulos inline/externos, microtasks, eventos e timer zero.
- [Recusas async](../../fixtures/document-lifecycle/unsupported-async.html): marcadores de efeito proibido para clássico externo async/async+defer e módulo async.
- [Roteiro e ordem esperada](../../fixtures/document-lifecycle/README.md): expectativas de código-fonte, não resultados observados.

As fixtures não foram executadas. Leitura/edição não equivale a compilação, teste, revisão consolidada ou validação funcional. Evidências posteriores devem ser registradas separadamente; checks anteriores não validam este recorte.

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram (processo de check com exit 0, 1,65 s). Permaneceu somente o warning anterior de Stmt::Function nunca construído. Há 11 regressões em código-fonte, não executadas. Não houve testes, execução de fixtures/aplicativo ou Clippy. Revisão textual consolidada concluída, incluindo a correção do preflight integral, as 11 regressões-fonte, as fixtures e a documentação, sem bloqueadores restantes. Compilação estática não comprova comportamento funcional.
