# WebAssembly i64 ↔ BigInt: segundo recorte do item 2

Data: 2026-11-08. Estado: implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem validação funcional.

## Contrato deste incremento

A ponte síncrona stateless passa a admitir parâmetros e resultado i64, além de i32/f32/f64. O caminho `Val::I64` ↔ `WasmValue::I64` ↔ `Value::BigInt` preserva os 64 bits sem conversão intermediária por f64; valores acima de 2^53 não são arredondados. O resultado JavaScript é BigInt com interpretação signed de 64 bits.

Argumentos i64 usam ToBigInt: BigInt, booleanos e strings inteiras válidas são aceitos; Number é rejeitado com TypeError, mesmo inteiro. Objetos passam por ToPrimitive com hint `number`, respeitando efeitos e exceções observáveis, antes da conversão. A redução é módulo 2^64, dentro das quotas existentes de BigInt; `18446744073709551615n` retorna `-1n`. Isso não torna entradas arbitrariamente grandes livres de limites nem acrescenta operações BigInt à plataforma inteira.

Continua havendo no máximo 32 parâmetros e zero ou um resultado. Assinaturas de referência e módulos com imports, memórias, tabelas, start, globals mutáveis, segmentos data/element ou exports não função continuam inelegíveis. A validade binária não implica elegibilidade de Instance. Não há persistência de estado Wasm: cada chamada cria Store efêmero, mantendo apenas módulos/Engine imutáveis compartilhados e identidade JavaScript dos objetos.

## Recursos e fronteira de confiança

Preflight estrutural, quotas cumulativas, cópia real de BufferSource, fuel compartilhado com JavaScript, GC e limites de pilha permanecem no contrato anterior. Resource continua fatal, restaura DOM/VM e preserva orçamento consumido. O retorno i64 cobra um tick e 8 bytes cumulativos antes de alocar BigInt; se essa cobrança causar Resource, o fuel já debitado continua consumido. Erros ordinários de coerção permanecem capturáveis e não desfazem efeitos DOM anteriores. Não há novo IPC, acesso a arquivos/rede, WASI, JIT, imports JavaScript, Promise/streaming ou WebAssembly completo.

## Fontes preparadas, não executadas

- `crates/web-wasm/src/lib.rs`: tipos/assinaturas e transporte i64 do interpretador.
- `crates/web-javascript/src/wasm.rs`: conversão i64/BigInt e fontes de regressão da ponte.
- `crates/web-bindings/src/wasm_i64_tests.rs`: DocumentSession real com binário embutido, valor exato 9007199254740993 no DOM, wrap signed, string/bool, hint number observável, rejeição Number e rollback DOM/VM por Resource na coerção.
- [Fixture-fonte HTML](../../fixtures/javascript-webassembly-i64.html): export identidade i64 e saída textual exata esperada, sem rede nem recursos externos.

Revisão textual consolidada do núcleo, integração DOM, fixture e documentação concluída sem bloqueadores de código. Checagem estática central aprovada: `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked`; sessão 40949, saída 0, duração informada de 1,87 s. Permanece apenas o warning preexistente de `Stmt::Function` em `parser.rs:179`.

Esses comandos formatam/verificam fontes e checam os alvos, mas não executam testes nem validam o comportamento. Nenhum teste, JavaScript, Wasm, fixture, aplicativo, navegador, rede, inferência, Clippy ou build de distribuição foi executado neste recorte. Não há evidência funcional. O conteúdo anterior de [primeira passagem](2026-11-08-roadmap-first-pass.md) permanece histórico; evidências do recorte Array.from não foram alteradas. `dist/Noir` não foi reconstruído; não houve commit, push ou publicação.

Itens 1-18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Este incremento do item 2 não reabre uma sequência de incrementos JavaScript nem conclui WebAssembly.

- [Primeiro recorte e quotas](2026-11-08-stateless-webassembly.md)
- [ADR 0016](../adr/0016-bounded-stateless-webassembly.md)
- [Contrato JavaScript](../specifications/javascript-subset.md)
