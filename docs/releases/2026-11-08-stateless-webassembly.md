# Item 2 — Primeiro recorte WebAssembly sem estado persistente

Data: 2026-11-08. Estado: em implementação parcial, revisão textual concluída e checagem estática aprovada. Sem validação funcional.

O recorte JavaScript 20 foi encerrado separadamente. O item 1 permanece ABERTO/parcial; este é o primeiro recorte do item 2, não um incremento 21 nem a conclusão do suporte WebAssembly.

## Escopo

`web-wasm` encapsula wasmi 2.0.0 oficial, interpretador sem JIT. A VM JavaScript continua própria. As dependências foram obtidas e registradas no Cargo.lock. Features: std/validate/stable/portable-dispatch/extra-checks; inspeção estrutural com wasmparser 0.228. Sem wat, memory64, simd, unstable, WASI ou CLI. O lint de código próprio não significa dependências livres de unsafe.

A ponte síncrona prevê validate, Module(BufferSource), Instance(Module[, imports]) e exports de funções i32/f32/f64, até 32 parâmetros e zero ou um resultado. Há interpretação real, loops, chamadas internas e traps. O conjunto de módulos binariamente válidos é maior que o conjunto elegível para Instance: um módulo pode validar e compilar, mas ter sua instanciação recusada explicitamente.

Entradas usam slots internos de BufferSource, offsets e comprimentos reais, com cópia cobrada antes de alocar. A cópia compilada não acompanha mutações posteriores do buffer. Objetos forjados não adquirem autoridade de BufferSource. Instâncias e funções exportadas têm identidade e vínculos rastreados pelo GC.

## Estado, orçamento e erros

O [ADR 0016](../adr/0016-bounded-stateless-webassembly.md) preserva o rollback do ADR 0007: Module/Engine imutáveis compartilháveis, identidade JS persistente e Store efêmero por chamada somente em módulos provadamente sem estado mutável persistente. Imports, memória, tabelas, start e globals mutáveis são recusados. Não há Store mutável compartilhado entre snapshots nem reinstanciação que simule persistência.

Fuel é debitado do orçamento JS também quando há trap. Esgotamento é Resource fatal e conserva o consumo durante rollback; erros ordinários de compilação, ligação, execução e tipo permanecem capturáveis. Bytes, estruturas, objetos, parâmetros/resultados, profundidade e instruções têm limites antes de alocações. 
Limites do backend e da ponte, cumulativos onde indicado:

| Recurso | Teto |
| --- | --- |
| Binário individual | 256 KiB, sujeito também aos limites de BufferSource da VM |
| Bytes copiados por validate/Module | 2 MiB cumulativos por sessão, inclusive tentativas inválidas |
| Module / Instance | 64 tentativas de Module que chegaram à compilação / 128 reservas de alocação de Instance elegível por sessão |
| Tipos / funções definidas / exports / globals definidos / imports | 128 / 256 / 128 / 128 / 128 por módulo |
| Assinaturas | 32 parâmetros e um resultado, inclusive tipos internos |
| Locais adicionais / corpo | 256 locais por função, além dos parâmetros; 16 KiB por corpo |
| Operadores / controle aninhado | 32.768 por módulo / profundidade 64 |
| Chamadas / pilha wasmi | Profundidade 32 / 16 KiB (16.384 bytes, não slots) |
| Store efêmero | Uma instância, zero memórias e zero tabelas |

EnforcedLimits::strict de wasmi também se aplica. Os limites de heap, strings, instruções e profundidade da VM podem interromper antes dos tetos acima. A cópia de bytes debita comprimento + 1 do orçamento JS antes de alocar. Quotas estruturais excedidas são Resource, inclusive em validate, não simplesmente false. A quota de Module é debitada antes de compilar, inclusive para binários inválidos; a quota de Instance é reservada antes de alocar uma instância elegível. Bytes contam tanto validate como Module. GC e rollback não repõem essas quotas cumulativas. Contagens são políticas locais de recusa, não quantidades de objetos vivos nem promessas de capacidade simultânea.

São permitidos valores internos i32/i64/f32/f64 e globals numéricos imutáveis. i64 na interface JS, exports que não sejam funções, segmentos data/element, imports, memória, tabelas, start, globals mutáveis e referências ficam inelegíveis. Reference types, multi-value, multi-memory, tail call, extended const, custom page sizes e wide arithmetic estão desligados. A compilação é eager: validação/compilação não fica adiada até a primeira chamada.

Module e Instance exigem new. Imports pode ser omitido, undefined ou objeto; para módulos elegíveis sem imports, suas propriedades/getters não são consultados. null e demais primitivas são TypeError. Funções exportadas não são construtoras. Aliases de um mesmo índice de função compartilham identidade dentro da mesma Instance; name é o índice decimal da função. Argumentos são convertidos em ordem por ToNumber, depois i32/f32/f64; i32 usa redução módulo 2^32, f32 arredonda ao formato, zero resultados retorna undefined. O objeto exports tem protótipo null, não é extensível e seus campos são somente leitura/não configuráveis. Instance.exports é propriedade própria somente leitura, não enumerável e não configurável, em vez do accessor padrão: uma limitação explícita. Isso não declara reflexão WebAssembly completa.

Binário inválido faz validate retornar false e Module lançar CompileError; BufferSource inválido lança TypeError. Instance de módulo fora do recorte lança LinkError. Traps ordinárias são RuntimeError; falta de fuel ou estouro de pilha são Resource fatais. CompileError, LinkError e RuntimeError são expostos no namespace, mas a fixture confere name, não instanceof. Os erros possuem protótipos próprios, sem cadeia Error completa.

## Ausências explícitas

- compile/instantiate com Promises e compilação/carregamento por streaming.
- Imports JavaScript, WebAssembly.Memory/Table/Global, estado mutável persistente e start.
- Interface i64/BigInt, referências e propostas não habilitadas.
- WASI, I/O, rede, processos/threads da página, novo IPC e aquisição de bytes via fetch().arrayBuffer().
- Protótipos/reflexão completos, conformidade geral, desempenho e segurança auditada.

## Evidências

A [fixture-fonte](../../fixtures/javascript-webassembly.html) registra expectativas de validação, Module/Instance, soma 42, views com offset, independência da cópia e recusas. Não foi executada. Expectativas não são resultados observados.

Revisão textual final concluída, sem falhas concretas pendentes, incluindo a verificação antes de alocar tipos/recgroups, aliases por índice de função e preservação da ordem binária dos exports. `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram na sessão 3861, exit 0, em 7,26 s. Permanece somente o aviso anterior de Stmt::Function.

Os testes-fonte foram compilados, não executados. As fontes DOM registram as expectativas `true:42:true:function` e, no caso de esgotamento, changed = 0 com scripts desativados e orçamento consumido preservado; não são resultados observados. Não houve execução de teste, fixture, WASM, aplicativo, Clippy, Test262/WPT, navegação ou validação visual. Não há validação funcional.

Não houve build release, reconstrução de `dist/Noir`, commit ou publicação. Os itens 1 e 2 permanecem ABERTOS/parciais; o item 19 do roadmap, testes, continua adiado.
