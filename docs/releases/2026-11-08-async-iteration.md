# Roadmap: iteração assíncrona limitada do item 1

Data: 2026-11-08. Décimo quarto incremento. Estado: implementação parcial no código-fonte, sem validação funcional. O item 1 permanece aberto.

## Sintaxe e protocolo

- `for await...of` é aceito somente em async functions, arrows e métodos async, inclusive definidos em módulos. O topo de scripts e módulos, funções síncronas, generators síncronos, `for await...in` e a forma de três cláusulas são recusados.
- Declarações var/let/const, padrões de declaração e referências admitidas pelo subconjunto participam da coleta de var, das verificações de conflito e do modo strict. Os bindings lexicais preservam escopo por iteração.
- `Symbol.asyncIterator` é um símbolo conhecido com identidade estável e distinta de `Symbol.iterator`. Seu método tem prioridade. Somente ausência/null permite fallback ao protocolo síncrono; método presente inválido não é ignorado.
- O objeto iterador e seu método next são capturados uma vez. Cada next é chamado com o iterador como receiver, e seu resultado é aguardado antes da leitura de done/value. O resultado deve ser um objeto.
- No protocolo async direto, value não recebe um await implícito adicional. Quando done é true, value não é lido por esse caminho.
- O fallback Async-from-Sync usa o iterador síncrono real e jobs de Promise próprios, em vez de simular suspensão com um snapshot geral. Lê done e value do resultado síncrono e aguarda value, inclusive com done=true, antes de produzir um novo IteratorResult. Arrays/strings conservam os caminhos incrementais existentes; coleções que usam IteratorRecord::Values conservam seus snapshots limitados.

## Fechamento e continuação

Break, return e falhas de binding/corpo fecham o iterador conforme a conclusão pendente. O método return é procurado no fechamento; quando existe, seu resultado é aguardado e deve ser um objeto. Continue da própria iteração prossegue sem fechar. Conclusão normal e falhas ao obter next/result não inventam um fechamento extra.

Um throw original prevalece sobre falhas ordinárias de fechamento. Para outras conclusões, uma falha de fechamento pode substituí-las. No adaptador síncrono, rejeição de value antes de done fecha o iterador síncrono e preserva a rejeição original diante de falhas ordinárias de fechamento. Erros de recurso continuam fatais, fora dessa precedência de exceções JavaScript.

Iteradores, métodos capturados, valores, conclusões pendentes e reações auxiliares participam dos roots de GC. Cancelar uma ativação remove suas continuações/reactions/jobs auxiliares, incluindo assimilação de thenables pertencentes à ativação, sem cancelar indiscriminadamente Promises do usuário. Isso não introduz uma API pública de cancelamento.

## Limites e ausências

- Até 4096 itens por loop; não é uma garantia de conseguir consumir 4096 itens em uma execução.
- Permanecem os limites compartilhados de 256 máquinas resumíveis, 512 registros de Promise e 1024 jobs, além de profundidade, instruções e memória. A adaptação e os awaits consomem esses recursos e podem atingir outro limite antes do limite de itens.
- O incremento não acrescenta async generators, yield* assíncrono, top-level await, avaliação assíncrona de grafos de módulos ou novas posições gerais de suspensão.
- Não amplia a iteração a primitivos arbitrários, o tratamento de Host/Native ou os protocolos completos de todos os exóticos. As strings e coleções suportadas continuam sujeitas ao subconjunto existente.
- Não acrescenta rede, acesso a arquivos, streams ou um event loop Web completo. Não incorpora motor JavaScript externo.

## Entrega

Na integração deste incremento, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram. O check compilou também as fontes de testes, sem executá-las. Permaneceu o aviso anterior da variante `Stmt::Function` não construída. Isso não constitui validação funcional.

A fixture [javascript-async-iteration.html](../../fixtures/javascript-async-iteration.html) contém exemplos pequenos de fallback síncrono, valor async sem await adicional e fechamento aguardado. Ela não foi executada. Testes-fonte foram preparados no código para sintaxe, protocolo, fechamento, limites e ciclo de vida; não foram executados. Não há comprovação funcional ou de compatibilidade com sites.

Este incremento não reconstrói `dist/Noir`, não gera build release e não publica uma distribuição. Veja também [funções resumíveis](2026-11-08-bigint-resumable.md) e a [matriz de capacidades](../architecture/capabilities.md).

## Incremento posterior

O [décimo quinto incremento, async generators limitados](2026-11-08-async-generators.md), acrescenta async function* e métodos async *, filas FIFO de next/throw/return e consumo por este for await...of. A ausência de async generators registrada acima descreve o escopo original do décimo quarto incremento, não o estado posterior. A ausência de yield* assíncrono corresponde ao estado daquele incremento; o [décimo sexto incremento](2026-11-08-async-delegation.md) acrescenta delegação assíncrona limitada, em implementação parcial e sem validação funcional. Top-level await continua ausente. Essa ampliação não transforma as checagens históricas em validação funcional do novo recurso.
