# Inspeção das propriedades existentes de Promise

O operador `in` passa a reconhecer `prototype` e os sete métodos estáticos já expostos pelo construtor Promise: resolve, reject, withResolvers, all, race, allSettled e any. Leitura e existência usam um inventário compartilhado, evitando que uma propriedade legível seja recusada pela inspeção.

A consulta é apenas de metadados. Não materializa o protótipo preguiçoso, não aloca Promises, não executa getters ou microtasks e não chama o host. Ela permanece dentro da caminhada limitada de protótipos e consome instruções. A conversão observável de chave e o teto de 1024 bytes continuam ativos.

## Escopo preservado

Não foram acrescentados métodos Promise nem autoridade genérica sobre Native ou Host. Descritores, Reflect e escrita/deleção do construtor Promise continuam sujeitos às restrições anteriores. O protótipo Promise já existente é um objeto da VM e conserva seu comportamento. Object.create com Promise como protótipo e subclasses de Promise continuam fora do subconjunto. Os limites de Promises, jobs, heap, strings, chamadas, parser e arrays não mudaram.

A fixture sintética `fixtures/javascript-promise-inspection.html` resolve um caminho por `in`, chega a Promise.prototype.finally e confirma a execução de microtasks por uma atualização real do DOM. Ela não carrega recursos externos e não demonstra compatibilidade geral com sites.

## Validação focal Linux

- `cargo test -p web-javascript -p web-bindings --locked --offline`: 527 testes da VM e 190 de bindings passaram, sem falhas ou ignorados. O recorte inclui oito testes novos de VM e dois de bindings.
- `cargo clippy -p web-javascript -p web-bindings --all-targets --locked --offline -- -D warnings`, `cargo fmt --all --check` e `git diff --check`: aprovados.
- `cargo run -p browser-cli --locked --offline -- render fixtures/javascript-promise-inspection.html --output /tmp/noir-promise-inspection.json`: zero warnings; display list confirmou “Inspeção e microtasks Promise ativos.”
- Os testes cobrem identidade do protótipo, métodos conhecidos/ausentes, inspeção sem heap/string budget, coerção e exceções de chave, getters sem execução durante consulta, caminhada interna até Native, invariantes de autorização e rollback fatal de DOM/globais/jobs pendentes. Revisão independente do desenho sem bloqueadores.

Essa validação focal não substitui os checks integrados do workspace nem a QA do navegador em sites externos.

## Validação integrada

Concluída em 2026-11-09: 1011 testes passaram, zero falharam e um teste de inferência real ficou ignorado por exigir modelo/runtime verificados. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` e `git diff --check` passaram. Os 976 arquivos do snapshot permaneceram iguais durante as verificações. Após a validação, apenas este registro documental foi acrescentado; código, testes e fixture não mudaram.
