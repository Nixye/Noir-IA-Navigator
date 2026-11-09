# Roadmap: BigInt e funções resumíveis do item 1

Data: 2026-11-08. Estado: **implementação parcial, sem validação funcional**.

Este incremento segue [módulos estáticos](2026-11-08-static-modules.md). O item 1 permanece aberto, e os demais itens do roadmap não estão concluídos.

## BigInt próprio

Inteiros com sinal em limbs de 32 bits, sem converter a aritmética inteira em ponto flutuante. A implementação impõe limite explícito de 4096 bits e cobra trabalho do orçamento da VM.

- Literais decimais, binários, octais e hexadecimais; conversão BigInt, Number e String no subconjunto.
- Soma, subtração, multiplicação, divisão truncada, resto, potência, operadores bitwise e deslocamentos com sinal.
- BigInt.asIntN/asUintN com truncamento exato em complemento de dois, largura até 4096 bits e conversão ToBigInt sem aceitar Number implicitamente.
- Comparações com Number sem arredondar previamente o BigInt, igualdade, typeof, atualização e representação em bases 2 a 36.
- Mistura numérica implícita com Number e conversões numéricas proibidas geram erro. APIs de strings, arrays, datas, buffers, RegExp e formatação receberam guardas de conversão; BigInt continua permitido como texto e elemento.
- JSON.stringify recusa BigInt sem transformação apropriada. Divisão por zero e expoentes negativos geram RangeError; estouro de orçamento continua sendo erro de recurso.
- Object(1n) produz wrapper BigInt com identidade própria. BigInt.prototype expõe constructor, valueOf, toString e Symbol.toStringTag; o próprio protótipo não contém o slot BigIntData. Os métodos validam o receiver e os wrappers participam do GC. JSON consulta toJSON e replacer antes de recusar um BigInt ainda presente.

O [incremento de arrays tipados](2026-11-08-bigint-typed-arrays.md) acrescenta BigInt64Array/BigUint64Array e os quatro métodos BigInt de DataView, com bytes exatos e truncamento módulo 2^64.

Limitações: formatação locale ainda não existe. Descritores de funções nativas e coerção de todas as APIs permanecem parciais; [operadores e chaves de propriedade](2026-11-08-operator-coercion.md) usam o novo caminho observável, enquanto caminhos antigos não representáveis ainda recusam wrappers explicitamente. Não há alegação de conformidade de todas as APIs numéricas ECMAScript.

## Async e generators em subconjunto

Foi criada uma máquina explícita de continuação. Cada suspensão conserva escopo, sequência e ação pendente; a retomada não percorre novamente o trecho já executado.

- `async function`, async arrows e `function*` em declarações/expressões apropriadas e exports de módulos estáticos. Arrows preservam this/arguments léxicos. Métodos async e generator síncrono de objetos/classes também são aceitos, incluindo static e privados de classe. Métodos comuns não são construtores; async() continua podendo ser um método comum chamado async.
- Await/yield diretos em expressão isolada, inicializador de variável simples, retorno, throw e condições de if/while/do-while e fases diretas de for clássico. Sequências, blocos, branches, loops e break/continue não rotulados são preservados. No for resumível, let recebe ambiente próprio por iteração e var conserva o binding compartilhado de função. Expressões sem suspensão continuam no avaliador normal.
- For-of síncrono e for-in admitem suspensão no corpo e fonte direta await/yield. A fonte é obtida uma vez, o próximo item não é repetido e os bindings let/const têm ambiente por iteração. For-of retém o iterador, o next capturado e valores necessários para GC; break/return/throw fecham o iterador, continue local não fecha. Erro original de throw prevalece sobre falha comum de fechamento; erros de recursos continuam fatais.
- Atribuições simples/compostas admitem RHS com expressões suspensíveis, incluindo binários, condicionais e cadeias de atribuição nas posições habilitadas. Alvo, chave e valor anterior de atribuição composta são obtidos uma vez antes de avaliar o RHS inteiro; coerção binária e escrita ocorrem somente após sua conclusão normal. Throw/return descartam a escrita pendente. GC conserva referência, receiver, valor anterior e classe privada necessários.
- Operadores binários, incluindo in/instanceof, &&/||/?? e vírgula, condicional ?: e unários +, -, ~, !, void e typeof aceitam operandos com suspensão nas posições habilitadas. Frames Evaluate/Apply conservam valores parciais e ordem dos efeitos; curto-circuito não avalia o ramo dispensado. Coerção usa os mesmos helpers do avaliador comum depois de completar os operandos. typeof de identificador preserva o tratamento especial de nome ausente.
- Await/yield também podem ter operandos que suspendem. A suspensão externa só é emitida depois da conclusão normal do operando interno; uma conclusão abrupta interna descarta a ação externa e segue para finally/IteratorClose. Await de um valor primitivo continua sendo um checkpoint próprio, não uma operação síncrona disfarçada.
- Chamadas comuns como fn(await x) e objeto[chave](yield x) aceitam argumentos suspensíveis sem spread. O alvo e o receiver são capturados antes dos argumentos, os argumentos seguem esquerda para direita, e o método não é lido novamente após retomada. Call/apply/bind e chaves Symbol usam o dispatch compartilhado com chamadas síncronas. O caminho nomeado legado de objetos Host não vira um getter artificial. GC conserva callable/receiver/argumentos já avaliados; throw/return descarta a chamada. O argumento 33 é recusado antes de seus efeitos, depois dos primeiros 32.
- Switch admite suspensão direta no discriminante e nos corpos das cláusulas. O escopo lexical único é preparado depois do discriminante; cases são consultados com igualdade estrita somente até a primeira correspondência, default é fallback e fallthrough não reavalia cases. Break local encerra o switch; continue e outros abrupts seguem para os frames externos.
- Async inicia imediatamente e retorna Promise intrínseca. Await sempre retoma via job FIFO, inclusive para valor simples ou Promise resolvida; thenables usam a assimilação existente.
- Try/catch/finally podem suspender. Rejeições de await e throw injetado entram no catch; finally conserva um return/throw durante suspensão, e uma nova conclusão abrupta o substitui. GC rastreia esses valores salvos.
- Retorno, rejeição e falhas de parâmetros de async resolvem/rejeitam a Promise. Exaustão de recursos continua fatal e não se torna uma rejeição capturável que contorne o orçamento.
- Generators expõem next/throw/return e Symbol.iterator. O corpo só começa em next; o primeiro argumento de next é ignorado. Valores seguintes são injetados no yield, e a conclusão permanece concluída. Retomada reentrante é recusada.
- Yield* síncrono delega ao iterador real: captura next uma vez, começa com um argumento undefined e encaminha next/throw/return. Resultado done=false é devolvido por identidade, sem ler value; done=true lê value e distingue retorno injetado de conclusão normal. Throw ausente fecha o iterador com conclusão normal antes de lançar TypeError, permitindo que erro de fechamento prevaleça. Return done=false não inventa retorno pendente. GC conserva iterador, último resultado e ação externa; no máximo 4096 retomadas por delegação. Arrays/strings usam seus caminhos incrementais existentes; coleções que chegam ao IteratorRecord::Values conservam o snapshot limitado anterior.
- Async e generators não são construtores. Async não recebe a propriedade própria prototype de uma função comum; generators têm o protótipo de instância limitado.
- GC rastreia escopos suspensos somente por owners alcançáveis. Reações/jobs de Promise mantêm a execução aguardada viva; estados inalcançáveis podem ser coletados. Valores parciais e ações pendentes participam do rastreamento. Máximo de 256 máquinas e profundidade combinada de 256 frames/ações/pending por máquina, além dos limites existentes de Promise e heap.

## Limites deliberados

Suspensões no callee, chamadas com spread, argumentos de eval direto/super/métodos privados/construção, objetos/classes, tagged templates, referências de propriedade, delete/update, lado esquerdo de atribuição, padrões de desestruturação, expressões de case e labels são recusadas na validação, antes de efeitos do script. Esses comandos podem aparecer dentro da função quando não contêm suspensão. Os argumentos de ImportCall continuam recusando await/yield, mesmo que o resultado de import possa ser aguardado. Não há simulação síncrona de await, replay do corpo, nem suporte silencioso parcial nesses pontos.

For-in usa o snapshot limitado de até 4096 chaves já empregado pelo motor: propriedades deletadas são puladas, mas propriedades adicionadas depois não entram. Alterações dinâmicas completas de enumerabilidade/protótipo ainda não são representadas. For-of continua usando apenas iteradores síncronos, mesmo quando seu corpo usa await.

Init de for com desestruturação ainda é recusado em funções resumíveis. O incremento posterior de [iteração assíncrona](2026-11-08-async-iteration.md) acrescenta for await...of e Async-from-Sync limitados. O décimo quinto incremento posterior acrescenta [async generators limitados](2026-11-08-async-generators.md), reutilizando essa máquina, com fila FIFO de requests e await de yield/return. O décimo sexto incremento posterior acrescenta [yield* assíncrono limitado](2026-11-08-async-delegation.md), em implementação parcial, sem validação funcional. Continuam pendentes top-level await e avaliação assíncrona de grafos de módulos. A [importação dinâmica registrada](2026-11-08-dynamic-import.md) posterior aceita await import(...) em funções originadas de módulos, mas não suspensão dentro dos argumentos de import. Super dentro de métodos de objetos é recusado conservadoramente; métodos de classes usam os hooks de home/private existentes. O tratamento completo de protótipos e metadados de funções nativas também permanece parcial. Isso não equivale ao event loop Web completo.

## Checagens e entrega

`cargo check --workspace --all-targets --offline --locked` passou durante a integração, incluindo compilação das fontes dos testes. Não houve execução de testes, fixture, navegador, Test262, WPT, Clippy ou validação visual.

Foram preparados testes-fonte de aritmética/conversões BigInt, coerções, ordenação de async, retenção de escopos, next/throw/return, não construtibilidade e rejeição antecipada de casos não suportados. A fixture `fixtures/javascript-resumable.html` não foi executada.

Sem build release, atualização de `dist/Noir`, empacotamento, commit, push ou publicação. A versão do pacote existente não foi alterada.

## Atualização de alcance: incremento 17

O incremento posterior de [literais resumíveis](2026-11-08-resumable-literals.md) acrescenta arrays/templates e operandos de spread de array nas posições habilitadas, sem ampliar as fronteiras proibidas acima. Elementos/partes são conservados sem replay e com orçamento antecipado. Estado desse incremento: implementação parcial, revisão textual concluída e checagem estática aprovada, sem validação funcional. Os resultados históricos de checagem deste documento não validam essa ampliação.


Nota de continuidade (incremento 18): as recusas históricas de suspensão em leituras Member e callees ordinários descritas nesta entrega foram ampliadas pelo [incremento de acessos resumíveis](2026-11-08-resumable-access.md). Isso não amplia LHS/assign/update/delete, private/super/new/eval direto nem as demais fronteiras ali registradas. As evidências desta entrega não validam o incremento 18.

Nota de continuidade (incremento 19 do item 1): o [incremento de atribuições resumíveis](2026-11-08-resumable-assignment.md) amplia as recusas históricas somente para base/chave de Member ordinário como LHS de atribuição simples/composta ou update ++/--. RHS suspenso em Name/private/super já era permitido no subconjunto e permanece; referências private/super suspensíveis e delete continuam fora. Revisão textual concluída e checagem estática aprovada para o incremento 19, sem validação funcional. As evidências históricas desta entrega não validam a ampliação; o item 1 permanece ABERTO e o item 19 do roadmap, testes, continua adiado.

Nota de continuidade (incremento 20 do item 1): o [delete Member resumível](2026-11-08-resumable-delete.md) amplia a recusa histórica somente para await/yield na base/chave de Member ordinário sob delete. Delete genérico, Name e referências private/super suspensíveis continuam fora. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. Evidências anteriores não validam esta ampliação. Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O avanço ao item 2 está em planejamento; ele não foi implementado nem concluído por esta entrega.


