# ADR 0024: Scripts inline dinâmicos síncronos e limitados

## Decisão

O preparo do ADR 0023 passa a aceitar fontes inline em elementos criados por `document.createElement('script')`. A inserção de um script com texto executa seu código antes de `appendChild`, `insertBefore` ou `replaceChild` retornar. Um script conectado ainda vazio pode ser preparado por `text`, `textContent`, `innerHTML` do próprio elemento script ou inserção/alteração de seu nó de texto. Scripts produzidos dentro de fragmentos `innerHTML` continuam inertes; não há reativação de scripts já iniciados, incluindo clones.

`Runtime::execute_nested_script` é uma entrada de script global, não eval nem Function. Não captura variáveis locais do chamador. Diretivas strict pertencem à fonte inserida e o estado anterior é restaurado em sucesso ou erro. Parsing completo antecede efeitos. A entrada não coleta lixo, não executa microtasks/finalizadores e não inicia uma nova transação: usa a VM, DOM, instruções, memória e orçamento da operação atual.

## Reentrância e checkpoints

Há teto de oito scripts aninhados, além dos limites de chamadas/expressões já existentes; um limite menor pode interromper antes. O contador e o frame de chamada são liberados mesmo em erro. GC também reconhece scripts aninhados ativos. O novo hook de setter com acesso ao runtime permite a inserção síncrona sem transformar atribuições em uma fila artificialmente assíncrona.

Checkpoints de callbacks exigem ausência de chamadas, expressões, eval, scripts aninhados e execução resumível ativos. Isso impede que um `window.onerror` invocado por uma atribuição DOM execute microtasks antes de o JavaScript pai terminar. A conclusão da avaliação externa sinaliza ao host antes do checkpoint.

`document.currentScript` identifica scripts clássicos iniciais, externos dinâmicos e inline dinâmicos. Inserções aninhadas guardam/restauram o script anterior; o valor é null durante microtasks posteriores, callbacks de carga e fora da avaliação. Módulos não ganham currentScript. A API ainda não implementa toda a interface HTMLScriptElement/WebIDL.

## Erros e limites

Fontes inline e externas compartilham 32 preparos dinâmicos, 128 KiB por fonte e 512 KiB cumulativos por documento. O texto é contado antes de concatenar a fonte. Scripts vazios ficam elegíveis para uma preparação posterior. Fontes inline não emitem load/error de rede.

Exceções ordinárias de sintaxe/execução inline são registradas em avisos limitados e comunicadas a window.onerror; não são lançadas pelo método DOM para o try/catch do script que inseriu o nó. Efeitos anteriores persistem. Resource continua fatal, não capturável, com rollback da operação externa e desabilitação de scripts/callbacks/pedidos. Falhas não renovam o orçamento de instruções.

Os lotes de scripts de uma subárvore capturam a fonte no preparo. Uma inserção reentrante conclui seu próprio lote antes de retomar o lote externo. Esse subconjunto não é todo o algoritmo de inserção/event loop do HTML Standard. Módulos dinâmicos, import com novas buscas, async paralelo, CSP/SRI completos e coordenação de cargas externas com DOMContentLoaded/window.load permanecem ausentes.

## Fronteira e validação

Não há novo I/O, autorização de URL, esquema IPC ou permissão de sandbox. Fontes externas continuam exclusivamente pelo broker do ADR 0023. One-shot e replay executam inline, mas permanecem sem novas buscas externas; capture não executa scripts.

Testes da VM e bindings cobrem declaração global, isolamento strict, ordem síncrona, currentScript aninhado e microtasks, setters conectados, alias/replace/subárvores, clones inertes, erros ordinários, exaustão de recursos, recursão e quotas compartilhadas. Testes de subprocesso exercitam DOM/VM retidos e a fronteira offline. Nada disso declara Google Search, YouTube ou aplicações arbitrárias funcionais.
