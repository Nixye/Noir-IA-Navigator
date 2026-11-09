# Roadmap: literais resumíveis limitados do item 1

Data: 2026-11-08. Décimo sétimo incremento. Estado: implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional. O item 1 permanece aberto.

## Escopo e ordem observável

Arrays e templates não marcados podem conter await/yield nas posições já habilitadas em funções async, generators e async generators. Há ações explícitas para elementos/partes, sem reexecutar o prefixo ao retomar. Literais aninhados herdam a permissão do contexto externo; inserir um array/template numa posição proibida não torna essa posição suspensível.

- Elementos e interpolações seguem esquerda para direita. Uma conclusão abrupta descarta o buffer parcial e a ação pendente, seguindo catch/finally externos sem publicar um literal incompleto nem inventar IteratorClose.
- Elisions do array são holes, diferentes de uma posição presente com undefined. Spread de uma fonte com holes produz os valores fornecidos por seu iterador, sem copiar a marca de ausência para o destino.
- O operando de spread pode suspender, por exemplo `[0, ...await source, 3]`; depois de obtê-lo, a expansão consome o iterador síncrono. Não há await de cada item nem suporte novo a Symbol.asyncIterator nesse caminho.
- O literal tem até 4096 posições, incluindo holes. A largura sintática acima do limite é recusada antes do primeiro elemento. Durante spread, um destino cheio interrompe após obter o primeiro item excedente, sem consumir o restante ou executar limpeza JavaScript após erro fatal de recursos.

## Helpers comuns e mudança deliberada de semântica

O avaliador comum e a máquina resumível usam literal_builders.rs para montar arrays/templates. Spread de array agora propaga falhas de next e de leitura de done/value sem chamar return. Essa mudança deliberada também se aplica aos arrays comuns; não modifica iterable_values nem os demais consumidores que o utilizam. Não confundir com os protocolos de fechamento de for-of, for await ou yield*.

Templates comuns e resumíveis usam ToString falível com o Host, hint string e os hooks Symbol.toPrimitive/toString/valueOf do subconjunto. A conversão de cada substituição termina antes de avaliar a próxima parte. Symbol em conversão implícita causa TypeError, inclusive quando retornado por um hook; String(Symbol) explícito conserva sua exceção existente. BigInt decimal é convertido com orçamento de trabalho, sem passar por Number. Unidades UTF-16, pares e surrogates isolados são preservados. Isso não amplia a representação dos objetos Host/nativos ou as APIs ainda recusadas pela coerção.

## Memória, GC e limites

Buffers de array reservam slots antes de cada crescimento, inclusive holes e valores de spread. A conclusão transfere o buffer ao heap sem cobrar esses slots novamente. O limite cumulativo de 65.536 slots por VM continua compartilhado com os outros consumidores e não é reembolsado quando a avaliação aborta.

Templates cobram dois bytes por unidade UTF-16 do buffer temporário antes do crescimento, além da representação final, cujo orçamento cumulativo é conferido antes da alocação. Reservas são cumulativas, sem reembolso em conclusões abruptas. O buffer aceita até 65.536 unidades UTF-16, mas o resultado deve caber em 64 KiB na representação real: UTF-8 quando possível ou UTF-16 se necessário. Não se aplica prematuramente o teto final de bytes a cada prefixo, pois um surrogate isolado posterior pode mudar a representação. O orçamento cumulativo de 4 MiB de strings e o trabalho em instruções podem interromper antes desses máximos.

Valores parciais, fonte/iterador/valores transitórios do spread e continuação externa participam das raízes de GC, inclusive durante chamadas observáveis. As raízes temporárias de objetos são deduplicadas por ID; o limite global de 4096 raízes ainda pode interromper casos com muitos objetos distintos ou outras raízes ativas. A capacidade de 4096 posições do array não garante capacidade para 4096 objetos distintos em qualquer contexto. Permanecem profundidade combinada de 256 frames/ações/pending por máquina, 256 máquinas compartilhadas, limites de heap, instruções, Promises e requests já documentados. Nenhum máximo garante capacidade simultânea. Erros de recursos permanecem fatais e não capturáveis.

## Fronteiras preservadas

Não habilita suspensão em objetos/classes, callee, construção com new, referências member ou LHS, padrões de desestruturação, tagged templates, spread de chamada, argumentos de eval direto/super/métodos privados/import, cases, labels ou demais posições recusadas. Não acrescenta top-level await, módulos assíncronos, carregamento sob demanda, rede/I/O/streams ou conformidade ECMAScript geral. O motor permanece próprio, sem acesso direto a arquivos ou rede.

## Entrega e evidências

A revisão textual foi concluída, incluindo as correções do preflight cumulativo antes de alocar a representação final do template e da deduplicação de raízes temporárias. `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram após essas correções. O check compilou os testes-fonte sem executá-los; permaneceu apenas o aviso anterior da variante `Stmt::Function` não construída. Isso não constitui validação funcional. Nenhum teste, fixture, navegador, Test262, WPT, Clippy ou validação visual foi executado nesta etapa. Os testes-fonte preparados não são resultados observados. A fixture [javascript-resumable-literals.html](../../fixtures/javascript-resumable-literals.html) prepara exemplos pequenos de holes/spread aguardado, ordem de conversão, retomada de generator, Symbol e UTF-16; não foi executada.

Sem build release, reconstrução de dist/Noir, empacotamento, publicação ou commit. Consulte a [matriz de capacidades](../architecture/capabilities.md), o [subconjunto JavaScript](../specifications/javascript-subset.md) e os limites de [funções resumíveis](2026-11-08-bigint-resumable.md).


Nota de continuidade (incremento 18): as recusas históricas de suspensão em leituras Member e callees ordinários descritas nesta entrega foram ampliadas pelo [incremento de acessos resumíveis](2026-11-08-resumable-access.md). Isso não amplia LHS/assign/update/delete, private/super/new/eval direto nem as demais fronteiras ali registradas. As evidências desta entrega não validam o incremento 18.

Nota de continuidade (incremento 19 do item 1): o [incremento de atribuições resumíveis](2026-11-08-resumable-assignment.md) amplia as recusas históricas somente para base/chave de Member ordinário como LHS de atribuição simples/composta ou update ++/--. RHS suspenso em Name/private/super já era permitido no subconjunto e permanece; referências private/super suspensíveis e delete continuam fora. Revisão textual concluída e checagem estática aprovada para o incremento 19, sem validação funcional. As evidências históricas desta entrega não validam a ampliação; o item 1 permanece ABERTO e o item 19 do roadmap, testes, continua adiado.

Nota de continuidade (incremento 20 do item 1): o [delete Member resumível](2026-11-08-resumable-delete.md) amplia a recusa histórica somente para await/yield na base/chave de Member ordinário sob delete. Delete genérico, Name e referências private/super suspensíveis continuam fora. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. Evidências anteriores não validam esta ampliação. Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O avanço ao item 2 está em planejamento; ele não foi implementado nem concluído por esta entrega.


