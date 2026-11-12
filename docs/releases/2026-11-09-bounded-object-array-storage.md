# Pacote de armazenamento limitado de objetos e arrays

Este pacote aceita dicionários ordinários com centenas de nomes e arrays de alguns milhares de posições, mantendo limites agregados e recusas fatais. Não remove quotas para fazer uma página passar.

- Propriedades: até 4096 nomes por owner ordinário, 65536 reservas cumulativas por VM, 8 MiB de bytes de chaves contabilizados e 4 Mi unidades de trabalho. Preflight, ordem e enumeração usam o mesmo orçamento. Hosts, símbolos e owners exóticos preservam suas restrições.
- Arrays: até 8192 posições por array sob o mesmo agregado de 65536 posições. Holes usam bitmap compacto, no máximo 1024 bytes de payload por array; valores continuam densos.
- Resultados de métodos reservam posições antes de crescer. Getters/callbacks não podem consumir posições já reservadas; resultados variáveis são cobrados por item. forEach deixa de construir um vetor descartado de resultados.
- Delete, GC e rollback não devolvem reservas cumulativas. Parser, strings, heap, instruções padrão e quotas independentes de iteração/enumeração permanecem.
- Ajustes de atomicidade evitam publicar cache de Array.prototype antes da reserva e inserir metadados de length após erro fatal de recurso.

## Validação focal Linux

- JavaScript: 556 testes aprovados, zero falhas.
- Bindings: 190 testes aprovados, zero falhas.
- Renderer: 16 testes aprovados, zero falhas.
- Clippy dos quatro crates web-javascript, web-bindings, web-renderer e browser-cli, com all-targets e warnings negados: aprovado.
- Fmt e diff-check: aprovados.
- Duas fixtures locais renderizadas pelo CLI com zero warnings: arrays exibem `PASS: length, holes, undefined and join`; propriedades exibem `OK:300:299:0`.

Esses resultados não substituem o agregado do workspace nem o QA Windows e não demonstram compatibilidade completa de sites. Testes de métodos que combinam muitas operações usam orçamento explícito do host; os limites padrão do produto não foram ampliados para acomodá-los.

Contratos: [ADR 0025](../adr/0025-cumulative-property-and-array-storage.md), [propriedades](../specifications/property-budgets.md), [arrays](../specifications/array-storage.md).
