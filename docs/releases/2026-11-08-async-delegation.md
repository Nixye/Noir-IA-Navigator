# Roadmap: delegação assíncrona limitada do item 1

Data: 2026-11-08. Décimo sexto incremento. Estado: implementação parcial do código-fonte, sem validação funcional. O item 1 permanece aberto.

## Protocolo de yield* async

O incremento amplia os [async generators](2026-11-08-async-generators.md) com yield* nas posições de suspensão admitidas pela máquina resumível. O operando é avaliado uma vez; Symbol.asyncIterator tem prioridade, e somente sua ausência/null permite recorrer a Symbol.iterator pelo adaptador Async-from-Sync. Não há replay do corpo nem materialização geral do iterável.

- O iterador e next são capturados uma vez. Throw e return são consultados dinamicamente a cada operação correspondente; um método presente não chamável gera erro.
- Next/throw/return preservam o iterador como receiver. A primeira chamada next da delegação recebe um argumento undefined; as retomadas encaminham um argumento, inclusive undefined. For await...of conserva chamadas next com zero argumentos. Fechamento por throw ausente usa return com zero argumentos.
- O resultado de cada operação é aguardado, deve ser um objeto e tem done lido antes de value. Diferentemente do for await direto, yield* lê value também com done=true.
- No protocolo async direto, o value obtido não recebe await implícito adicional na delegação. Com done=false, ele entra em um novo IteratorResult do async generator, sem preservar a identidade do objeto IteratorResult delegado. Uma Promise armazenada em value continua sendo esse valor, sem ser assimilada apenas por estar nessa propriedade.
- Isso segue o ECMA-262 atual: AsyncGeneratorYield não aguarda seu argumento. O yield comum aguarda seu operando antes dessa operação; não se deve copiar esse await para yield* async.
- Async-from-Sync aguarda value, inclusive quando done=true, antes de produzir o resultado adaptado. Arrays/strings conservam seus caminhos incrementais; coleções representadas por IteratorRecord::Values conservam os snapshots limitados existentes.

## Conclusões e fechamento

Next ou throw delegado com done=true conclui normalmente a expressão yield* com value. Return delegado com done=true produz a conclusão de retorno. Return com done=false suspende a delegação normalmente: o retorno antigo não fica pendente e não é reaplicado à próxima retomada.

Quando throw está ausente, a delegação fecha o iterador com conclusão normal antes de produzir TypeError. No protocolo async direto, o resultado de return é aguardado e validado como objeto sem ler done/value. Uma falha de getter, chamada, rejeição ou resultado inválido desse fechamento prevalece sobre o TypeError de throw ausente. No fallback, o fechamento do iterador síncrono valida seu objeto de resultado sem await ou leitura de done/value/then. Não confundir esse caminho com o fechamento de for await causado por um throw do corpo, no qual o throw original prevalece sobre falhas ordinárias de fechamento.

Quando return está ausente no iterador async direto, há um Await adicional explícito do valor recebido antes de iniciar a conclusão abrupta e percorrer finally, mesmo que o request externo já tenha aguardado esse valor. No adaptador Async-from-Sync, return ausente produz seu resultado terminal sem assimilar diretamente o argumento; esse caminho não deve ser confundido com a continuação que aguarda value de um resultado síncrono existente.

Falhas ao obter ou aguardar resultados e ler suas propriedades seguem os handlers catch/finally da máquina. Elas não se transformam em uma chamada artificial ao throw do delegado. Os requests externos next/throw/return continuam na fila FIFO do async generator, sem intercalar execução durante um await pendente.

## GC, limites e fronteiras

Fonte, iterador, next capturado, valores transitórios e continuação externa são rastreados pelo GC nas fases pendentes. Getters e chamadas observáveis conservam raízes temporárias. Requests, Promises e jobs reutilizam o ciclo de vida e a limpeza da ativação existentes; não há API pública nova de cancelamento nem cancelamento indiscriminado de Promises do usuário.

- Até 4096 passos por delegação, incluindo encaminhamento de operações; não é garantia de consumir 4096 valores.
- Profundidade combinada de 256 frames/ações/pending por máquina e até 256 máquinas resumíveis compartilhadas.
- 256 identidades de async generator; 256 requests por gerador e 512 totais pendentes, incluindo resultados em assimilação.
- 512 registros de Promise e 1024 jobs/reactions compartilhados; awaits/adaptação também consomem esses recursos.
- 65.536 slots cumulativos de armazenamento por VM e 64 retomadas síncronas aninhadas na condução de async generators, além de instruções, heap, strings e profundidade.

Qualquer orçamento pode se esgotar antes do limite local; os máximos não garantem capacidade simultânea. Erros de recursos continuam fatais, sem virar rejeições capturáveis que contornem limites. Após exaustão, não há garantia de liquidação das Promises de requests restantes.

Não há top-level await, avaliação assíncrona de grafos de módulos, carregamento sob demanda, novas posições gerais de suspensão, rede, I/O, streams ou event loop Web completo. A delegação não torna async/generators ou ECMAScript completos. O motor permanece próprio e não ganha acesso direto a arquivos ou rede.

## Entrega e evidências

Na integração deste incremento, rustfmt, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram. O check compilou os testes-fonte sem executá-los; permaneceu somente o aviso anterior da variante `Stmt::Function` não construída. A revisão textual integrada de protocolo, máquina, GC e cancelamento foi concluída sem defeitos concretos identificados. Isso não constitui validação funcional. Nenhum teste, fixture, navegador, Test262, WPT, Clippy ou validação visual foi executado nesta etapa.

A fixture [javascript-async-delegation.html](../../fixtures/javascript-async-delegation.html) prepara exemplos pequenos de value async direto preservado, fallback síncrono com Promise e encaminhamento de return com done=false. Ela não foi executada. Os resultados descritos na página são expectativas, não resultados observados.

Sem build release, reconstrução de dist/Noir, empacotamento, publicação ou commit. Consulte a [matriz de capacidades](../architecture/capabilities.md) e o [subconjunto JavaScript](../specifications/javascript-subset.md).

Referências normativas: [AsyncGeneratorYield](https://tc39.es/ecma262/multipage/control-abstraction-objects.html#sec-asyncgeneratoryield) e [avaliação de definições de generator, incluindo yield*](https://tc39.es/ecma262/multipage/ecmascript-language-functions-and-classes.html#sec-generator-function-definitions-runtime-semantics-evaluation). A descrição delimita o subconjunto implementado; não declara conformidade integral.

## Atualização de alcance: incremento 17

O incremento posterior de [literais resumíveis](2026-11-08-resumable-literals.md) amplia as posições documentadas para arrays/templates e operandos de spread de array, também nos corpos de async generators. A expansão do spread continua síncrona após a conclusão do operando; ela não é yield* nem for await. As demais fronteiras de suspensão continuam recusadas. Esse incremento está em implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; não altera as evidências históricas desta entrega.
