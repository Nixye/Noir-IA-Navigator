# Roadmap: importação dinâmica registrada do item 1

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

O parser de módulos aceita import(specifier), import(specifier, options) e vírgula final. A expressão pode ser usada em funções originadas do módulo e em await import(...) nas posições já suportadas. A base de resolução continua sendo a origem lexical, mesmo quando a função roda depois de outro módulo. Import estático continua sendo uma declaração distinta.

## Execução e ordem

- Especificador e options são avaliados uma vez, antes da criação da Promise. Erros dessas avaliações continuam síncronos.
- A Promise é intrínseca e nova em cada chamada. ToString do especificador e leitura de options.with ocorrem durante a chamada; erros comuns dessas etapas rejeitam a Promise.
- Somente módulos previamente registrados podem ser encontrados. Grafo, link, avaliação única, falhas de evaluation e namespace são reutilizados. A resolução com o namespace usa a assimilação normal de thenables.
- Avaliação e conclusão são etapas FIFO internas separadas. Falhas de lookup/link rejeitam no primeiro job; sucesso ou falha de evaluation, incluindo falha cacheada, usam a continuação posterior. Não há replay dos argumentos, checkpoint público recursivo ou coleta de GC no núcleo da execução do módulo. Valores de jobs pendentes são rastreados; Resource permanece fatal.

## Fronteiras deliberadas

Não existe busca sob demanda nem preload de import literal. No documento, as fontes precisam ter sido registradas por um grafo estático autorizado, ou outro módulo já processado. Fontes clássicas, mesmo presentes nos assets, não são promovidas a módulos. Ausência gera erro genérico, sem revelar o identificador interno.

Scripts clássicos, eval e Function recusam a sintaxe. with diferente de undefined é recusado, inclusive um objeto vazio. Await/yield dentro dos argumentos de import ainda é recusado pela validação de posições de suspensão. Não há spread ou construção direta com new import.

Parsing ocorre no registro e falhas de link não ficam em um cache completo; falhas de avaliação ficam. Top-level await, avaliação assíncrona de grafos, import maps/import.meta, namespace exótico completo e equivalência geral de scheduling permanecem pendentes. Veja [ADR 0015](../adr/0015-registered-dynamic-import.md).

## Entrega

Cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram na integração. Essa checagem não comprova comportamento funcional.

Testes-fonte cobrem ordem de efeitos, promessas distintas, namespace estável, cache, base lexical, GC, assimilação de then e erros de recursos. A integração com web-bindings inclui um caso que recusa asset clássico como alvo dinâmico. A fixture javascript-modules demonstra import de uma dependência já registrada. Nenhum teste, fixture ou navegador foi executado.

Sem build release, atualização de dist/Noir, commit, push ou publicação. O item 1 continua aberto.
