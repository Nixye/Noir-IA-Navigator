# Roadmap: async generators limitados do item 1

Data: 2026-11-08. Décimo quinto incremento. Estado: implementação parcial no código-fonte, sem validação funcional. O item 1 permanece aberto.

Este incremento amplia a [máquina de funções resumíveis](2026-11-08-bigint-resumable.md) e a [iteração assíncrona](2026-11-08-async-iteration.md), sem incorporar motor JavaScript externo.

## Sintaxe e requests

- Async function* em declarações, expressões e exports; métodos async * de objetos/classes, incluindo static e privados admitidos pelo subconjunto. Métodos comuns chamados async() conservam sua interpretação.
- Await, yield e for await...of nos corpos desses geradores. Parâmetros e funções ordinárias aninhadas não herdam permissão para suspender. Modificadores inválidos de constructor/get/set e quebras de linha proibidas são recusados antecipadamente.
- A chamada cria um iterador assíncrono; o corpo começa no primeiro next, cujo argumento é ignorado. Next/throw/return retornam Promises intrínsecas e entram em fila FIFO por gerador. Requests posteriores não ultrapassam a suspensão ativa nem reexecutam o corpo desde o início.
- O iterador expõe next, throw, return e Symbol.asyncIterator. Receiver sem identidade interna de async generator rejeita a Promise de request com TypeError, em vez de iniciar uma máquina falsa. Esgotamento de recursos continua fatal, inclusive na alocação dessa Promise.
- Yield aguarda o operando antes de resolver a request com `{value, done: false}`. Uma rejeição retorna como throw ao corpo e pode atingir catch/finally. O valor recebido por next é injetado na expressão yield, sem await automático desse argumento.
- Return com expressão aguarda o operando antes de criar a conclusão abrupta e percorrer finally. Uma rejeição pode ser tratada pelo catch correspondente. Finally pode suspender e substituir a conclusão pendente.
- Return externo aguarda seu valor: antes do início ou após a conclusão, produz o resultado concluído sem executar o corpo; quando suspenso em yield, a resolução injeta return e a rejeição injeta throw. Requests seguintes permanecem na fila durante essa espera.
- For await...of consome esses iteradores pelo protocolo async existente. Break/return/throw no consumidor usam o fechamento aguardado; finally do gerador pode executar await durante o fechamento.
- As funções async generator não são construtores. Há protótipo limitado de instância; isso não implementa a cadeia completa de protótipos ECMAScript nem um construtor público AsyncGeneratorFunction.

## Memória, cancelamento e limites

O rastreamento de GC conserva as duas direções entre o gerador e suas Promises de requests pendentes, os valores enviados, a Promise aguardada e os escopos/frames da continuação. As tabelas de identidade não mantêm sozinhas todos os geradores vivos; registros inalcançáveis são removidos antes do reuso de IDs.

Falhas fatais removem a continuação, a fila e os auxiliares pertencentes à ativação afetada, sem cancelar indiscriminadamente Promises do usuário ou outros geradores. Não há API pública de cancelamento. Erros de recurso não se tornam rejeições JavaScript capturáveis que contornem o orçamento. A limpeza fatal pode deixar Promises de requests pendentes, sem garantia de liquidação após exaustão.

A assimilação de um IteratorResult com then herdado conserva uma associação separada com o gerador até a Promise ser liquidada. Ela não bloqueia requests seguintes, mas permite cancelar a ativação e os resolvers correspondentes se esse job sofrer uma falha fatal. Return/throw normais de um gerador já concluído não cancelam resultados anteriores ainda em assimilação.

- 256 identidades de async generator retidas e 256 máquinas resumíveis compartilhadas com as demais funções resumíveis.
- 256 requests por gerador e 512 requests totais pendentes, incluindo resultados em assimilação; cada request consome também um registro de Promise.
- 512 registros de Promise compartilhados, além dos auxiliares de await e encadeamentos que usam esse mesmo orçamento.
- 1024 jobs/reactions conforme os limites compartilhados de Promise e microtasks; o checkpoint também permanece limitado.
- 65.536 slots de armazenamento cumulativos por VM, compartilhados com arrays e outros consumidores. Cada request cobra um slot, não reembolsado ao sair da fila.
- 64 retomadas síncronas aninhadas na condução de async generators, para limitar reentrada por falhas síncronas de PromiseResolve e callbacks observáveis.
- Permanecem profundidade, instruções, heap, strings e os limites das continuações. Outro orçamento pode terminar antes desses máximos; eles não garantem capacidade simultânea nem número de itens consumíveis.

## Ausências deliberadas

Yield* assíncrono é recusado explicitamente antes de avaliar seu operando; yield* síncrono permanece no caminho existente. Não há top-level await, avaliação assíncrona de grafos de módulos ou event loop Web completo. As posições gerais de suspensão continuam limitadas às documentadas na máquina resumível. Não se acrescentam rede, arquivos ou streams ao renderer.

## Checagens e entrega

Na integração deste incremento, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram. O check compilou as fontes dos testes sem executá-las. Permaneceu o aviso anterior da variante `Stmt::Function` não construída. Isso não constitui validação funcional.

Foram acrescentados testes-fonte de sintaxe, requests FIFO, await de yield/return, fechamento, receptores inválidos, GC e erros fatais. Nenhum teste foi executado. A fixture [javascript-async-generators.html](../../fixtures/javascript-async-generators.html) prepara exemplos pequenos de fila de next e break com finally aguardado; ela não foi executada. Não houve navegação, Test262, WPT ou validação visual deste recurso.

Sem build release, reconstrução de `dist/Noir`, empacotamento ou publicação. Consulte a [matriz de capacidades](../architecture/capabilities.md) e o [subconjunto JavaScript](../specifications/javascript-subset.md).

## Incremento posterior

O [décimo sexto incremento, delegação assíncrona limitada](2026-11-08-async-delegation.md), acrescenta yield* async em implementação parcial. A recusa descrita acima registra o escopo original do décimo quinto incremento. O caminho novo aguarda IteratorResult, mas não aguarda implicitamente o value de um iterador async direto; Async-from-Sync aguarda value. As checagens históricas acima não validam esse novo incremento. Top-level await e avaliação assíncrona de módulos permanecem ausentes, sem validação funcional do novo recurso.

## Atualização de alcance: incremento 17

O incremento posterior de [literais resumíveis](2026-11-08-resumable-literals.md) amplia as posições documentadas para arrays/templates e operandos de spread de array, também nos corpos de async generators. A expansão do spread continua síncrona após a conclusão do operando; ela não é yield* nem for await. As demais fronteiras de suspensão continuam recusadas. Esse incremento está em implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; não altera as evidências históricas desta entrega.
