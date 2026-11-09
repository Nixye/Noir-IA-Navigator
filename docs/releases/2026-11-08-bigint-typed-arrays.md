# Roadmap: arrays tipados BigInt do item 1

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

Este incremento amplia o sistema de buffers do motor próprio. Não incorpora outro runtime e não altera o isolamento de arquivos/rede.

## Implementado no subconjunto

- BigInt64Array e BigUint64Array usam os mesmos ArrayBuffer limitados do motor. Leitura e escrita de 64 bits são exatas, sem passagem do valor inteiro por f64. O tipo com sinal interpreta complemento de dois; a escrita trunca módulo 2^64.
- DataView.getBigInt64/getBigUint64/setBigInt64/setBigUint64 respeitam offset e ordem de bytes explícita, com big-endian por padrão.
- Construtores, from/of, indexação, set, slice/subarray, fill, at, includes/indexOf, join, values/keys/entries, forEach/map e reverse foram integrados à representação Value. Métodos existentes continuam sendo o subconjunto suportado, não uma API completa de typed arrays.
- ToBigInt usa conversão de objeto com hint number e recusa Number implícito. Typed arrays numéricos continuam recusando BigInt. Cópias diretas entre famílias numérica/BigInt são recusadas, inclusive quando a fonte está vazia; comparação de busca não converte o valor buscado.
- Atribuição de elemento efetua conversão antes de descartar índice inválido ou fora dos limites. Definição de descritor valida as restrições do índice antes de converter o valor, preservando a ordem própria dessa operação.
- Set guarda snapshot de fontes typed array para preservar sobreposição; fontes array-like intercalam leitura, conversão e escrita. Construtor/from distinguem iterável coletado de array-like; from valida mapper antes de consumir a fonte e converte cada resultado antes do próximo callback. Join preserva separadores UTF-16 e consulta seus métodos de conversão.
- Fontes e buffers retidos continuam rastreados pelos mecanismos existentes do GC. Permanecem os limites de 4096 elementos, 1 MiB por buffer e 4 MiB agregados de buffers, além dos orçamentos de execução e valores.

## Limites

Não há SharedArrayBuffer, Atomics, buffers redimensionáveis/destacáveis nem semântica completa de species/subclasses ou reflexão exótica. APIs não existentes antes deste recorte não passam a existir somente porque o novo tipo está presente. Conversão de Host/Native e objetos especiais não representáveis segue sendo recusada.

Os limites de BigInt de 4096 bits continuam válidos antes do truncamento de armazenamento. O código não alega conformidade ECMAScript geral nem compatibilidade com aplicações arbitrárias.

## Entrega

Após integração e formatação, cargo check --workspace --all-targets --offline --locked passou. Essa checagem compila as fontes, incluindo testes-fonte, mas não os executa e não comprova comportamento funcional.

Testes-fonte cobrem fronteiras signed/unsigned, conversões, descritores, offsets, endian, sobreposição, buscas e retenção. A fixture javascript-bigint-typed-arrays.html foi preparada. Nenhum teste ou fixture foi executado. O executável distribuído não foi reconstruído; não houve release, empacotamento, commit, push ou publicação. O item 1 e os demais itens do roadmap continuam abertos conforme seus estados documentados.
