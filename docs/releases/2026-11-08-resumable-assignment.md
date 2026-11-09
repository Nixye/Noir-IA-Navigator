# Roadmap: atribuições e updates resumíveis limitados do item 1

Data: 2026-11-08. Décimo nono incremento do item 1, não o item 19 do roadmap. Estado: implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional. O item 1 permanece ABERTO; os testes do item 19 continuam adiados.

## Referência capturada e ordem observável

O incremento amplia await/yield na base e na chave computada de Member ordinário usado como LHS de atribuição simples/composta ou de update ++/--, nas posições habilitadas de funções async, generators e async generators. A referência é formada uma vez: base → avaliação da expressão-chave → rejeição de base nullish → ToPropertyKey. A retomada não repete o prefixo e Symbol conserva sua identidade. A expressão-chave ainda é avaliada quando a base é null/undefined; nesse caso sua conversão e o RHS não ocorrem. Um erro da expressão-chave prevalece sobre o TypeError da base.

Na atribuição simples, a referência capturada agenda o RHS sem ler o valor antigo nem chamar getter. Na composta, GetValue lê o valor antigo uma vez antes do RHS; as conversões do operador acontecem somente depois da conclusão normal do RHS. A escrita usa o destino capturado, mesmo se o RHS substituir a variável-base ou modificar a propriedade.

Update segue GetValue → ToNumeric uma vez → passo Number/BigInt → PutValue. Prefix devolve o novo valor; postfix devolve o valor numérico antigo já convertido, não o objeto/string original. Essa ordem também corrige a conversão duplicada no caminho comum. A formação comum de referências Member também rejeita nullish depois da avaliação da chave, mas antes de ToPropertyKey e do RHS.

## Conclusões abruptas, memória e limites

Throw, return ou rejeição durante uma suspensão não completam a escrita pendente. Finally segue o tratamento normal da máquina; a escrita descartada não é retomada depois dele. Erros de recursos continuam fatais, não capturáveis.

O rastreamento de GC preserva base, chave ainda não convertida, referência capturada, valor antigo e continuação externa entre suspensões. Os temporários do update durante avaliação/conversão ativa são protegidos pelo bloqueio de coleta, sem exigir um token de raiz individual para cada valor. Permanece a guarda do incremento 18: não há coleta explícita durante execução ativa de owner resumível, inclusive em callbacks Host diretos com profundidades zeradas. Continuam o teto global de 4096 raízes deduplicadas, 256 máquinas compartilhadas, 256 frames/ações/pending combinados e os orçamentos existentes de heap, instruções, strings, BigInt, Promises e armazenamento. Os máximos não garantem capacidade simultânea.

## Fronteiras preservadas

RHS suspenso de atribuições a Name e a referências private/super já era permitido no subconjunto; não é novidade deste incremento e não foi retirado. A novidade é a formação suspensível da referência Member ordinária. Não habilita referências private/super suspensíveis, delete, desestruturação/padrões, construção com new, objetos/classes, argumentos de import ou eval direto, spread de chamada, cases, labels, tagged templates ou top-level await. &&=, ||= e ??= continuam ausentes do parser.

Não amplia strict mode, escritas em primitivas/Host, descritores ou a semântica geral de PutValue além do subconjunto já suportado. Também não declara async/generators completos, módulos assíncronos, carregamento sob demanda ou conformidade ECMAScript. Motor próprio e fronteiras de arquivos/rede permanecem inalterados.

## Entrega e evidências

Revisão textual final concluída, incluindo os 11 novos testes-fonte, sem achados concretos. A formatação com rustfmt foi aplicada; `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram. O check terminou com código 0 em 2,30 segundos e compilou os testes-fonte sem executá-los; permaneceu apenas o aviso anterior da variante `Stmt::Function` não construída. Isso não constitui validação funcional. Nenhum teste, Clippy, fixture, navegador, Test262, WPT ou validação visual foi executado neste incremento. A fixture [javascript-resumable-assignment.html](../../fixtures/javascript-resumable-assignment.html) prepara exemplos de destino capturado async, ordem da composta, update em generator, Symbol e base nullish. Seus resultados descritos são expectativas, não observações.

A nova fonte de integração DOM em web-bindings espera `assigned:compound:4:6`. Trata-se de expectativa de fonte não executada, não de resultado observado.

Sem build release, reconstrução de dist/Noir, empacotamento, publicação ou commit. Checagens dos incrementos anteriores não validam este incremento. Consulte a [matriz de capacidades](../architecture/capabilities.md), o [subconjunto JavaScript](../specifications/javascript-subset.md) e o incremento anterior de [acessos resumíveis](2026-11-08-resumable-access.md).

Nota de continuidade (incremento 20 do item 1): o [delete Member resumível](2026-11-08-resumable-delete.md) amplia a recusa histórica somente para await/yield na base/chave de Member ordinário sob delete. Delete genérico, Name e referências private/super suspensíveis continuam fora. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. Evidências anteriores não validam esta ampliação. Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O avanço ao item 2 está em planejamento; ele não foi implementado nem concluído por esta entrega.


