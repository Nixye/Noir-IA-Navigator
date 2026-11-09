# ADR 0016 — WebAssembly limitado e sem estado persistente

Data: 2026-11-08. Estado: em implementação parcial; revisão textual concluída e checagem estática aprovada, sem validação funcional.

## Decisão e fronteira

O primeiro recorte do item 2 do roadmap acrescenta `web-wasm`, que encapsula o interpretador oficial `wasmi` 2.0.0, sem JIT. JavaScript continua sendo o motor próprio do Noir: não há browser engine incorporado nem substituição da VM JavaScript. O item 1 permanece ABERTO/parcial; este trabalho não é seu incremento 21.

A configuração seleciona somente `std`, `validate`, `stable`, `portable-dispatch` e `extra-checks`, com `wasmparser` 0.228 para inspeção estrutural. Não habilita `wat`, `memory64`, `simd`, `unstable`, WASI ou CLI. O lockfile registra as dependências obtidas. A API consumida pelo código próprio é segura e o lint `unsafe_code = forbid` aplica-se ao workspace; isso NÃO afirma que a cadeia de dependências é livre de `unsafe`.

## Por que instâncias sem estado

O ADR 0007 exige que esgotamento de recursos restaure DOM e VM anteriores à operação, sem restaurar orçamento consumido. O Runtime é clonável para esse rollback, mas o Store de wasmi não implementa Clone. Compartilhar Store mutável em Arc/Mutex entre snapshots quebraria essa garantia. Recriar Store em cada chamada para módulos com estado também seria incorreto: perderia memória, tabelas ou globals entre chamadas.

Módulos compilados e Engine imutáveis podem ser compartilhados. A identidade JavaScript da Instance e de suas funções exportadas permanece na VM, enquanto cada chamada usa Store efêmero somente depois de provar que o módulo não possui estado mutável persistente. A elegibilidade recusa imports, memórias, tabelas, start, globals mutáveis, segmentos data/element, exports que não sejam funções e assinaturas exportadas i64/referência. Valores internos i32/i64/f32/f64 e globals numéricos imutáveis são permitidos. A execução de funções é real, incluindo loops, chamadas internas e traps, com fuel. Não há mocks de resultado nem tentativa de simular persistência por reinstanciação.

Memórias, tabelas, imports e outras formas de estado futuro exigirão snapshot integral/restaurável do estado WASM ou redesenho explícito do ADR 0007. Não basta remover uma condição de elegibilidade.

## Contrato de entrada e execução

A primeira superfície é síncrona: `WebAssembly.validate(BufferSource)`, `new WebAssembly.Module(BufferSource)` e `new WebAssembly.Instance(module[, imports])`. Exportam-se funções numéricas i32/f32/f64 com no máximo 32 parâmetros e zero ou um resultado. A validade binária, sob a configuração habilitada, é separada da elegibilidade: validate pode retornar true e Module pode compilar um módulo que Instance recusa explicitamente por estar fora do recorte.

BufferSource é reconhecido por slots internos de ArrayBuffer/TypedArray/DataView, nunca por propriedades forjadas. A cópia considera byteOffset/byteLength reais e é cobrada antes de alocar; mutar o buffer original depois da compilação não altera o módulo. Quotas de bytes, estrutura, objetos, instruções e profundidade antecedem alocações e trabalho; GC marca os vínculos das instâncias e funções exportadas e varre registros não alcançáveis. Os limites concretos ficam na nota de implementação.

Fuel compartilha o orçamento restante de JavaScript; o consumo também é debitado em traps. Esgotamento é Resource fatal, não capturável nem substituível por finally. Erros ordinários de compilação, ligação, execução e tipo continuam distintos, sem prometer uma cadeia completa de protótipos WebAssembly.

## Autoridade e recusas

Não há WASI, I/O, acesso à rede, processos ou threads da página. Não há novo IPC, aquisição de bytes por URL ou `fetch().arrayBuffer()` utilizável. Não existem compile/instantiate baseados em Promise, streaming, imports JavaScript, memória/tabelas acessíveis, start, estado mutável persistente, interface i64/BigInt ou referências. A presença de um parser binário não declara suporte geral às extensões da plataforma.

## Evidência e próximos passos

Revisão textual final concluída sem falhas concretas pendentes; formatação e checagem estática do workspace e de todos os alvos aprovadas. Comandos, duração e aviso remanescente estão na nota de implementação. Testes-fonte apenas compilados; nenhum teste, fixture, WASM, aplicativo, Clippy, Test262/WPT, build de distribuição ou navegação executado para este recorte. Auditoria, fuzzing e conformidade WebAssembly/JS API permanecem pendentes. Os itens 1 e 2 continuam ABERTOS/parciais. `dist/Noir` não foi reconstruído; não houve commit nem publicação.

- [ADR 0007](0007-script-error-semantics.md)
- [Nota do primeiro recorte](../releases/2026-11-08-stateless-webassembly.md)
- [Features oficiais de wasmi 2.0.0](https://docs.rs/crate/wasmi/2.0.0/features)
- [Manifest oficial de wasmi 2.0.0](https://raw.githubusercontent.com/wasmi-labs/wasmi/v2.0.0/crates/wasmi/Cargo.toml)
- [Especificação WebAssembly](https://webassembly.github.io/spec/)

## Complemento de 2026-11-08: interface i64/BigInt

O segundo recorte do item 2 remove somente a recusa de i64 na interface exportada descrita acima. `Val::I64` ↔ `WasmValue::I64` ↔ `Value::BigInt` preserva exatamente os bits, sem f64 intermediário. ToBigInt recebe primitive com hint number, rejeita Number e reduz módulo 2^64 sob quotas; resultados têm interpretação signed. Referências continuam inelegíveis. Isso não cria estado Wasm persistente nem requer Store compartilhado: o mesmo Store efêmero, fuel, preflight e rollback do ADR 0007 permanecem. Nenhuma nova autoridade é concedida.

Revisão textual consolidada deste complemento concluída sem bloqueadores e checagem estática central aprovada; sem execução funcional. A aprovação histórica do primeiro recorte não é evidência desta extensão. Fontes e limites estão na [nota i64/BigInt](../releases/2026-11-08-webassembly-i64-bigint.md). Itens 1 e 2 continuam ABERTOS/parciais.
