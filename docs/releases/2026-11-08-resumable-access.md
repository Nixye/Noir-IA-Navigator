# Roadmap: acessos e callees resumíveis limitados do item 1

Data: 2026-11-08. Décimo oitavo incremento. Estado: implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional. O item 1 permanece ABERTO.

## Escopo e ordem observável

Leituras ordinárias de propriedade (Member) aceitam await/yield na base e na chave computada, nas posições habilitadas de funções async, generators e async generators. Chamadas ordinárias aceitam callee suspensível, incluindo métodos com base/chave suspensíveis e expressões que produzem uma função. Cada componente é avaliado uma vez; a retomada não repete o prefixo.

A ordem de um método computado é: base → avaliação da expressão-chave → verificação de base nullish → ToPropertyKey → leitura/getter → argumentos da esquerda para a direita → chamada. O receiver original e o alvo capturado antes dos argumentos são preservados, mesmo se argumentos suspensos substituírem a variável-base ou a propriedade. Getter que lança impede os argumentos; alvo não chamável produz TypeError somente depois dos argumentos. A chamada comum também lança um TypeError capturável para primitivos e objetos Host não chamáveis, em vez de erro genérico de runtime; a ponte de métodos Host nomeados LegacyMethod permanece separada.

A expressão-chave é avaliada mesmo com base null/undefined. Nesse caso sua coerção não ocorre: o erro da expressão-chave prevalece, mas hooks ToPropertyKey não são chamados antes do TypeError da base. A correção vale também para a leitura Member comum, que antes podia converter a chave cedo demais sobre uma base nullish. Chaves Symbol conservam a identidade. Métodos Host nomeados mantêm a ponte LegacyMethod existente, sem fabricar leitura de getter ou transformar APIs Host em propriedades JavaScript completas.

## Memória e limites

Base, chave ainda não convertida, alvo, receiver, argumentos acumulados e continuação externa participam das raízes de GC, inclusive em chamadas observáveis. A coleta explícita não realiza coleta enquanto qualquer owner resumível estiver em execução. Essa guarda também cobre callbacks Host diretos com profundidades de call/eval/dynamic iguais a zero, preservando os escopos e frames enquanto a máquina ativa está temporariamente fora do mapa de continuações. As raízes temporárias são deduplicadas por ID, dentro do teto global de 4096 raízes. O teto de 32 argumentos interrompe antes da avaliação do 33º; não amplia spread de chamada. Permanecem 256 frames/ações/pending combinados por máquina, 256 máquinas compartilhadas e os orçamentos de heap, instruções, strings, Promises e armazenamento. Os máximos não garantem capacidade simultânea; exaustão de recursos continua fatal e não capturável.

## Fronteiras preservadas

Não amplia suspensão em LHS/referências de atribuição, update/delete, private/super, construção com new, argumentos de eval direto, spread de chamada, objetos/classes, padrões, cases, labels, tagged templates ou argumentos de import. Um Member permitido como leitura não torna essa mesma expressão um LHS suspensível. Callee arbitrário pode alcançar eval indireto dentro das fronteiras existentes; não acrescenta eval direto. Não acrescenta top-level await, avaliação assíncrona de módulos, carregamento sob demanda, I/O/rede/streams ou conformidade ECMAScript geral. O motor permanece próprio, sem runtime incorporado nem acesso direto a arquivos ou rede.

## Entrega e evidências

Revisão textual concluída, incluindo as correções de TypeError em chamadas comuns e da guarda de coleta durante retomadas ativas. A formatação com rustfmt foi aplicada; `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram. O check terminou com código 0 em 2,06 segundos e compilou os testes-fonte sem executá-los; permaneceu apenas o aviso anterior da variante `Stmt::Function` não construída. Isso não constitui validação funcional. Nenhum teste, Clippy, fixture, navegador, Test262, WPT ou validação visual foi executado neste incremento. Os testes-fonte e a fixture [javascript-resumable-access.html](../../fixtures/javascript-resumable-access.html) são preparação para validação posterior, não resultados observados. A fixture contém exemplos pequenos de leitura async, receiver e alvo capturados, callee arbitrário, ordem de chave/getter/argumentos, base nullish e generator.

A regressão-fonte da coleta durante callback Host espera `answer === 7`; a fonte integrada de web-bindings cobre método/leitura do document Host e receiver ordinário com resultado esperado `42:3:kga:read`. São expectativas das fontes não executadas, não resultados observados.

Sem build release, reconstrução de dist/Noir, empacotamento, publicação ou commit. Consulte a [matriz de capacidades](../architecture/capabilities.md), o [subconjunto JavaScript](../specifications/javascript-subset.md) e o incremento anterior de [literais resumíveis](2026-11-08-resumable-literals.md). As checagens históricas do incremento 17 não validam o incremento 18.

Nota de continuidade (incremento 19 do item 1): o [incremento de atribuições resumíveis](2026-11-08-resumable-assignment.md) amplia as recusas históricas somente para base/chave de Member ordinário como LHS de atribuição simples/composta ou update ++/--. RHS suspenso em Name/private/super já era permitido no subconjunto e permanece; referências private/super suspensíveis e delete continuam fora. Revisão textual concluída e checagem estática aprovada para o incremento 19, sem validação funcional. As evidências históricas desta entrega não validam a ampliação; o item 1 permanece ABERTO e o item 19 do roadmap, testes, continua adiado.

Nota de continuidade (incremento 20 do item 1): o [delete Member resumível](2026-11-08-resumable-delete.md) amplia a recusa histórica somente para await/yield na base/chave de Member ordinário sob delete. Delete genérico, Name e referências private/super suspensíveis continuam fora. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. Evidências anteriores não validam esta ampliação. Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O avanço ao item 2 está em planejamento; ele não foi implementado nem concluído por esta entrega.


