# Ciclo de vida inicial: fixtures-fonte não executadas

Expectativas para avaliação posterior, não resultados de testes. Nenhuma fixture, script ou aplicação foi executada nesta entrega. Item 4 parcial.

## index.html

Quatro assets locais: classic.js, defer-first.js, module.mjs e defer-last.js. Sem rede, imports adicionais, fetch, TLA ou rAF. O trace esperado, incluindo checkpoints dos callbacks/scripts, é:

1. inline:loading
2. micro-inline:loading
3. classic:loading
4. promise-classic:loading
5. inline-async-defer:loading
6. readystatechange:interactive
7. micro-readystatechange:interactive
8. defer-first:interactive
9. micro-defer-first:interactive
10. module-external:interactive
11. micro-module-external:interactive
12. defer-last:interactive
13. micro-defer-last:interactive
14. module-inline:interactive
15. micro-module-inline:interactive
16. DOMContentLoaded:interactive
17. micro-DOMContentLoaded:interactive
18. readystatechange:complete
19. micro-readystatechange:complete
20. load:complete
21. micro-load:complete
22. timer:complete

O timer zero registrado no primeiro script não deve ultrapassar load. O clássico inline async/defer posterior permanece em loading, enquanto o módulo inline entra na fila após defer-last.

## unsupported-async.html

Clássicos externos async/async+defer e módulos async inline/externos devem gerar diagnósticos de capacidade não suportada, sem chamar mark. Result permanece “Nenhum efeito async permitido.”; lifecycle deve chegar a “load:complete” se não houver Resource fatal. Continuar após recusa não significa async implementado.

## Limites da observação

DOM inteiro disponível e assets em lote: sem parser bloqueante ou load universal. Não cobre idempotência da API Rust run_initial, rollback sob Resource, quotas máximas, transporte ou propagação completa. Esses cenários exigem verificação posterior específica; estas fixtures não os comprovam.
