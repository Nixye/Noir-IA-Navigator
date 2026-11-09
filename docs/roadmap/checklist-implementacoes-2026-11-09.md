# Checklist integral das implementações planejadas do Noir

Data: **2026-11-09**. Cobertura: **137 de 137 subitens**, com IDs estáveis e texto original preservado literalmente. **4 Implementados, 92 Parciais e 41 Pendentes. Nenhum dos 19 macrogrupos está concluído.**

Este arquivo é o inventário rastreável do escopo, não uma declaração de navegador pronto. O JSON de mesmo nome conserva os mesmos IDs, textos, estados, bases, lacunas, fontes e evidências.

## Como ler os marcadores

- **Implementado / [x]**: todo o contrato daquele subitem existe no pipeline delimitado. Limites e validação continuam explícitos; não significa conformidade Web geral.
- **Parcial / [ ]**: existe implementação concreta, mas ao menos uma parte do bullet original falta. O símbolo ✅ só identifica a capacidade concreta descrita na linha de implementação, nunca o bullet inteiro.
- **Pendente / [ ]**: a capacidade específica está ausente, fora do contrato ou não foi demonstrada nas fontes examinadas. Código relacionado não é contado como atendimento; “não verificado” não é prova universal de inexistência.
- **Validação é um eixo separado**. Fonte, teste escrito, compilação, teste aprovado e observação visual são evidências diferentes. Resultados globais não provam automaticamente cada requisito.
- **Origem** separa bases anteriores, incrementos de 08/11 e correções funcionais posteriores. Releases que diziam “sem testes” continuam corretas para sua entrega histórica.

## Evidência funcional atual e histórico

- **Ciclo 16 confirmado**: cargo test --workspace --offline --locked --no-fail-fast, sem skips, saída 0. **845 aprovados, 0 falhas, 1 ignorado e 0 filtrados**. Os quatro testes de integração AppContainer foram efetivamente executados nesse ciclo. [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [target/validation/2026-11-09-cycle16-tests.log.exitcode](../../target/validation/2026-11-09-cycle16-tests.log.exitcode).
- **Inferência real ainda não executada**: tests::bundled_model_produces_real_output permanece ignorado por exigir modelo/runtime verificados. Não houve --ignored nesse comando.
- **Fmt e Clippy aprovados**, ambos saída 0: [target/validation/2026-11-09-cycle16-fmt-after.log.exitcode](../../target/validation/2026-11-09-cycle16-fmt-after.log.exitcode); [target/validation/2026-11-09-cycle16-clippy-after.log](../../target/validation/2026-11-09-cycle16-clippy-after.log); [target/validation/2026-11-09-cycle16-clippy-after.log.exitcode](../../target/validation/2026-11-09-cycle16-clippy-after.log.exitcode).
- Contagens relevantes do ciclo 16: VM 440/440; bindings 130/130; CLI 26/26 (9 codec, 5 capture/replay e 12 subprocesso); CSS 36/36; DOM 3/3; HTML 9/9; layout 26/26; graphics 9/9; paint 7/7; renderer 15/15; Wasm 10/10; IPC 23/23; browser-process 18/18; browser-core 34/34; desktop unitário 35/35; subprocess 12/12 e worker_transport 2/2.
- **Histórico preservado**: ciclo 15 teve 797 aprovados/0 falhas/1 ignorado/0 filtrados ([target/validation/2026-11-09-cycle15-workspace-tests.log](../../target/validation/2026-11-09-cycle15-workspace-tests.log)); ciclo 12 teve 774 aprovados/11 falhas/1 ignorado/4 filtrados; ciclo 13 teve 792/1/1/4. Os quatro AppContainer depois aprovados em chamadas separadas (2 CLI + 2 desktop) não foram retroativamente somados aos comandos filtrados. [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md); [target/validation/2026-11-09-cycle13-tests.log](../../target/validation/2026-11-09-cycle13-tests.log); [target/validation/2026-11-09-appcontainer-cli.log](../../target/validation/2026-11-09-appcontainer-cli.log); [target/validation/2026-11-09-appcontainer-desktop.log](../../target/validation/2026-11-09-appcontainer-desktop.log).
- A fixture de snapshots do desktop que falhou no ciclo 13 passou no ciclo 15 e nas repetições posteriores: **24 chamadas (12 isoladas e 12 do grupo de quatro), 60 execuções de casos aprovadas, zero falhas**. Não são 60 testes novos. [target/validation/2026-11-09-snapshot-fixture-after.log](../../target/validation/2026-11-09-snapshot-fixture-after.log).
- **Smoke GUI/Google histórico do ciclo 13**: executável temporário do ciclo 13, SHA-256 66C535CCDE8AA1B0621EC6075E332E0B785A0E079D0C344C0A997C4F651A3CC5, perfil temporário. A fixture mostrou JS/DOM ok; Google mostrou logo e formulário, mas conservou erros _DumpException, atribuição src recusada e CSS ignorado. Sem busca submetida, login ou reprodução; naquele snapshot a causa inicial não estava estabelecida. O novo replay abaixo preservou os erros anteriores, sem corrigi-los. [target/validation/google-current/result.json](../../target/validation/google-current/result.json); [target/validation/google-current/diagnosis.md](../../target/validation/google-current/diagnosis.md).
- Test262, WPT, fuzzing, ensaio prolongado, inferência real e validação ampla de GPU/acessibilidade/segurança/compatibilidade continuam pendentes. Trabalho novo em Symbol/Math/performance está em andamento e não herda os resultados desse snapshot.

Os artefatos em **target/validation são evidências locais**, verificadas neste computador; podem não acompanhar o controle de versão. Fontes de teste e relatórios históricos não devem ser substituídos por alegações de aprovação futura.

### Incrementos diagnósticos validados no ciclo 16

- JavaScript conserva até quatro exceções anteriores e contextos internos limitados, sem alterar message, thrown ou Error.cause. Posições só são atribuídas quando o offset provém da fonte correta. Bindings acrescentam origem/fase/script/nó/tag/tipo e URLs saneadas; 17 testes de diagnóstico passaram. [docs/specifications/script-diagnostics.md](../../docs/specifications/script-diagnostics.md).
- CLI capture/replay explícito usa codec de até 12 MiB, mantém a URL HTTPS e AppContainer, não busca recursos no replay e não incorpora perfil/cookies/storage real. Foram aprovados 9 testes do codec e 5 de integração, além dos 12 de subprocesso. O snapshot não é autenticado nem criptografado e pode preservar dados privados da página. [docs/specifications/diagnostic-snapshots.md](../../docs/specifications/diagnostic-snapshots.md).
- O diagnóstico novo inicialmente provocou stack overflow na suíte e no caso isolado. A correção estrutural mantém metadados opcionais em Box, sem baixar o limite de parse 64 nem a entrada de profundidade 70. A regressão passou e a suíte JS terminou 440/440; isso não prova ausência universal de overflow. Histórico: [target/validation/2026-11-09-exception-diagnostics-crate.log](../../target/validation/2026-11-09-exception-diagnostics-crate.log); [target/validation/2026-11-09-exception-diagnostics-stack-repro.log](../../target/validation/2026-11-09-exception-diagnostics-stack-repro.log); depois [target/validation/2026-11-09-exception-diagnostics-stack-fixed.log](../../target/validation/2026-11-09-exception-diagnostics-stack-fixed.log); [target/validation/2026-11-09-exception-diagnostics-crate-after.log](../../target/validation/2026-11-09-exception-diagnostics-crate-after.log).
- **Google, captura/replay novos do ciclo 16**: somente homepage pública, capturada em 2026-11-09T02:16:05.2027058Z; CLI SHA-256 672A7D6F29D9497484C13BD010558AC7A1F5C5A99611FC57F5D5CF4050F8F3D7. Capture e replay retornaram 0. Script 2 preservou Host/native exotic descriptors are not implemented; scripts 8/9 preservaram Cannot access a property of null or undefined antes de _DumpException. Atribuição src foi recusada em node=112, tag=script, value-type=string. Nesse replay da homepage não houve envio de consulta, login ou reprodução, correção dessas incompatibilidades ou linha/coluna inventada. O teste GUI posterior de busca é descrito separadamente abaixo. [target/validation/google-diagnostics-cycle16/capture-session.json](../../target/validation/google-diagnostics-cycle16/capture-session.json); [target/validation/google-diagnostics-cycle16/google-home-replay.json](../../target/validation/google-diagnostics-cycle16/google-home-replay.json); [target/validation/google-diagnostics-cycle16/capture.log.exitcode](../../target/validation/google-diagnostics-cycle16/capture.log.exitcode); [target/validation/google-diagnostics-cycle16/replay.log.exitcode](../../target/validation/google-diagnostics-cycle16/replay.log.exitcode).
- **Busca/Voltar GUI posterior, ciclo 16: busca testada e falhando.** Cópia temporária SHA-256 58D65BF453DB1A0A928C535E650DABFEF07C40139738F60A1483998E6420DCAB, iniciada aproximadamente em 2026-11-09T02:24:31Z. A consulta sintética Noir browser test foi submetida e navegou a Google Search, mas nenhuma lista de resultados apareceu: somente mensagem de dificuldade. Warnings: Value is not callable (random), performance is not defined e seletores CSS ignorados; não são prova de causalidade completa. Voltar retornou à homepage com controles de histórico coerentes e campo vazio, sem provar BFCache/restauração de formulário. Sem segunda busca, login, CAPTCHA observado ou reprodução. Correções Symbol/Math/performance posteriores não integram o teste. [target/validation/google-search-cycle16/report.md](../../target/validation/google-search-cycle16/report.md); [target/validation/google-search-cycle16/result.json](../../target/validation/google-search-cycle16/result.json); [target/validation/google-search-cycle16/binary-and-session.json](../../target/validation/google-search-cycle16/binary-and-session.json).
- **Console continua separado**: console.* permanece no armazenamento interno limitado; esse recorte não o conecta ao inspector. Os ganhos são diagnóstico e reprodução, não conclusão de 18.5 ou compatibilidade Google.

## Cobertura por grupo

| Grupo | Subitens | Implementados | Parciais | Pendentes | Macrogrupo |
| --- | ---: | ---: | ---: | ---: | --- |
| 1. Motor JavaScript próprio | 8 | 0 | 7 | 1 | Aberto |
| 2. WebAssembly | 4 | 0 | 3 | 1 | Aberto |
| 3. HTML, documentos e DOM | 9 | 0 | 6 | 3 | Aberto |
| 4. Event loop e ciclo de vida | 6 | 0 | 6 | 0 | Aberto |
| 5. Eventos, entrada e edição | 7 | 0 | 4 | 3 | Aberto |
| 6. CSS e cascata | 8 | 0 | 6 | 2 | Aberto |
| 7. Layout e geometria | 9 | 0 | 9 | 0 | Aberto |
| 8. Texto e fontes | 6 | 0 | 4 | 2 | Aberto |
| 9. Rede e carregamento de recursos | 8 | 0 | 4 | 4 | Aberto |
| 10. Navegação e contextos de páginas | 7 | 1 | 4 | 2 | Aberto |
| 11. Persistência, workers e aplicações offline | 7 | 0 | 2 | 5 | Aberto |
| 12. Imagens e gráficos Web | 6 | 0 | 3 | 3 | Aberto |
| 13. Áudio, vídeo e streaming | 8 | 0 | 2 | 6 | Aberto |
| 14. Comunicação, identidade e dispositivos | 6 | 0 | 2 | 4 | Aberto |
| 15. Segurança e isolamento | 8 | 1 | 7 | 0 | Aberto |
| 16. Driver próprio de renderização do Noir | 11 | 2 | 8 | 1 | Aberto |
| 17. Automação e agente | 6 | 0 | 5 | 1 | Aberto |
| 18. Produto, acessibilidade e operação | 7 | 0 | 6 | 1 | Aberto |
| 19. Comprovação de compatibilidade | 6 | 0 | 4 | 2 | Aberto |
| **Total** | **137** | **4** | **92** | **41** | **0 de 19 concluídos** |

Os únicos marcadores principais concluídos são **10.7, 15.8, 16.1 e 16.4**. Nenhum percentual de prontidão do navegador é inferido dessas contagens: os subitens têm tamanhos e riscos diferentes.

## Inventário completo

### 1. Motor JavaScript próprio

- [ ] **1.1** Sintaxe e semântica ECMAScript: expressões, declarações, funções, classes, herança, campos privados, destructuring, templates, spread/rest e iteração.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Parser/AST e execução próprios para expressões, declarações, funções, classes, herança, campos privados, defaults/rest, destructuring, templates, spread e for...of, no subconjunto suportado.
  - Origem: Base própria preexistente + incrementos de linguagem/classes/destructuring e funções resumíveis de 08/11.
  - Lacunas/limites: Gramática e semântica ECMAScript integrais; contextos super/privados/Annex B e várias posições suspensíveis permanecem restritos.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro.
  - Fontes: [docs/roadmap/implementações Planejadas.md:73](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/parser.rs](../../crates/web-javascript/src/parser.rs); [crates/web-javascript/src/language.rs](../../crates/web-javascript/src/language.rs); [crates/web-javascript/src/class_runtime.rs](../../crates/web-javascript/src/class_runtime.rs); [crates/web-javascript/src/strict_validation.rs](../../crates/web-javascript/src/strict_validation.rs); [crates/web-javascript/src/resumable.rs](../../crates/web-javascript/src/resumable.rs); [docs/releases/2026-11-08-javascript-roadmap-item1.md](../../docs/releases/2026-11-08-javascript-roadmap-item1.md); [docs/releases/2026-11-08-resumable-delete.md](../../docs/releases/2026-11-08-resumable-delete.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **1.2** Escopos, closures, hoisting, TDZ, modo estrito, coerções e tratamento de exceções.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Bindings var/léxicos, closures, hoisting, TDZ, validação antecipada e modo estrito parcial; coerção falível Symbol.toPrimitive/valueOf/toString; exceções ordinárias versus Resource fatal com rollback.
  - Implementação concreta: ✅ Ciclo 16: histórico interno limitado de até quatro exceções anteriores e contextos inertes, sem alterar message, thrown ou Error.cause; marca de proveniência permite offset de parse somente quando pertence à fonte correta.
  - Origem: Base de escopos/exceções + coerções/strict/limites de 08/11 e correções dos ciclos funcionais. Ciclo 16 acrescenta diagnóstico interno e correção estrutural do armazenamento opcional em Box, sem baixar os limites existentes.
  - Lacunas/limites: Escopos, strict, Arguments Object/aliases, Annex B e coerções exóticas/Host completos; os limites call_depth=16 e oito coerções ativas não provam segurança/conformidade geral.
  - Lacunas/limites: Histórico não é stack trace nem causalidade JavaScript completa; Promise rejeitada e finally que substitui conclusão mantêm limites explícitos. Casos corrigidos não provam ausência universal de stack overflow.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro. O novo diagnóstico inicialmente provocou stack overflow reproduzido; armazenamento compacto opcional em Box corrigiu o caso mantendo o limite de parse 64 e a entrada de profundidade 70. Regressão focada e suíte de 440 passaram; falhas anteriores preservadas.
  - Testes exatos aprovados no ciclo 16: parser::resumable_syntax_tests::classical_for_yield_probe_keeps_resource_errors_fatal; exception_diagnostics::tests::failing_google_style_reporter_keeps_first_error_without_defining_reporter.
  - Fontes: [docs/roadmap/implementações Planejadas.md:75](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/language.rs](../../crates/web-javascript/src/language.rs); [crates/web-javascript/src/strict_validation.rs](../../crates/web-javascript/src/strict_validation.rs); [crates/web-javascript/src/coercion.rs](../../crates/web-javascript/src/coercion.rs); [crates/web-javascript/src/dynamic_code.rs](../../crates/web-javascript/src/dynamic_code.rs); [docs/releases/2026-11-08-operator-coercion.md](../../docs/releases/2026-11-08-operator-coercion.md); [crates/web-javascript/src/exception_diagnostics.rs](../../crates/web-javascript/src/exception_diagnostics.rs); [crates/web-javascript/src/lib.rs](../../crates/web-javascript/src/lib.rs); [docs/specifications/script-diagnostics.md](../../docs/specifications/script-diagnostics.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md); [target/validation/2026-11-09-exception-diagnostics-crate.log](../../target/validation/2026-11-09-exception-diagnostics-crate.log); [target/validation/2026-11-09-exception-diagnostics-stack-repro.log](../../target/validation/2026-11-09-exception-diagnostics-stack-repro.log); [target/validation/2026-11-09-exception-diagnostics-stack-fixed.log](../../target/validation/2026-11-09-exception-diagnostics-stack-fixed.log); [target/validation/2026-11-09-exception-diagnostics-crate-after.log](../../target/validation/2026-11-09-exception-diagnostics-crate-after.log).

- [ ] **1.3** Módulos, dependências, ciclos, bindings vivos, importação dinâmica e avaliação assíncrona.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Módulos estáticos com link de grafo, ciclos, execução única, imports imutáveis e bindings vivos; import dinâmico do registro autorizado e import.meta.url.
  - Origem: Incrementos de 08/11: módulos estáticos, import dinâmico registrado e import.meta.
  - Lacunas/limites: Carregamento dinâmico sob demanda, avaliação assíncrona/top-level await, import maps/attributes e namespace exótico completo.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro.
  - Fontes: [docs/roadmap/implementações Planejadas.md:77](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/parser_modules.rs](../../crates/web-javascript/src/parser_modules.rs); [crates/web-javascript/src/modules.rs](../../crates/web-javascript/src/modules.rs); [crates/browser-core/src/module_assets.rs](../../crates/browser-core/src/module_assets.rs); [docs/releases/2026-11-08-static-modules.md](../../docs/releases/2026-11-08-static-modules.md); [docs/releases/2026-11-08-dynamic-import.md](../../docs/releases/2026-11-08-dynamic-import.md); [docs/releases/2026-11-08-import-meta.md](../../docs/releases/2026-11-08-import-meta.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **1.4** Promises, `async`/`await`, generators e iteradores síncronos/assíncronos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Promises/thenables/combinadores e fila FIFO; async/await e generators com continuações reais; iteração síncrona/assíncrona, for await...of, async generators e yield* assíncrono limitados.
  - Origem: Base de Promises/timers + continuações, async iteration/generators e delegação de 08/11.
  - Lacunas/limites: Suspensão em todas as posições, top-level await, cadeias de protótipos completas, scheduler Web e rejection tracking.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro.
  - Fontes: [docs/roadmap/implementações Planejadas.md:79](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/promises.rs](../../crates/web-javascript/src/promises.rs); [crates/web-javascript/src/resumable.rs](../../crates/web-javascript/src/resumable.rs); [crates/web-javascript/src/resumable_functions.rs](../../crates/web-javascript/src/resumable_functions.rs); [crates/web-javascript/src/async_iteration.rs](../../crates/web-javascript/src/async_iteration.rs); [crates/web-javascript/src/async_generators.rs](../../crates/web-javascript/src/async_generators.rs); [docs/releases/2026-11-08-async-iteration.md](../../docs/releases/2026-11-08-async-iteration.md); [docs/releases/2026-11-08-async-generators.md](../../docs/releases/2026-11-08-async-generators.md); [docs/releases/2026-11-08-async-delegation.md](../../docs/releases/2026-11-08-async-delegation.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **1.5** Objetos nativos completos: arrays, strings, números, datas, coleções, símbolos, BigInt, buffers, arrays tipados, proxies e reflexão.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Arrays e Array.from, strings/números/Math/Date, Map/Set/Symbol, BigInt até 4096 bits, buffers/DataView/arrays tipados inclusive BigInt64; descritores/protótipos selecionados e 13 operações Reflect em alvos representáveis.
  - Origem: Base de objetos nativos + Reflect/BigInt/Math/números/arrays tipados de 08/11; segunda rodada Array.from.
  - Lacunas/limites: Proxy; reflexão, exóticos e metadados completos; Date local/locale, species, buffers redimensionáveis, SharedArrayBuffer e Atomics.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro.
  - Fontes: [docs/roadmap/implementações Planejadas.md:81](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/stdlib.rs](../../crates/web-javascript/src/stdlib.rs); [crates/web-javascript/src/array_methods.rs](../../crates/web-javascript/src/array_methods.rs); [crates/web-javascript/src/array_from.rs](../../crates/web-javascript/src/array_from.rs); [crates/web-javascript/src/bigint.rs](../../crates/web-javascript/src/bigint.rs); [crates/web-javascript/src/reflect_api.rs](../../crates/web-javascript/src/reflect_api.rs); [docs/releases/2026-11-08-reflect.md](../../docs/releases/2026-11-08-reflect.md); [docs/releases/2026-11-08-bigint-typed-arrays.md](../../docs/releases/2026-11-08-bigint-typed-arrays.md); [docs/releases/2026-11-08-array-from-dom.md](../../docs/releases/2026-11-08-array-from-dom.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **1.6** JSON, expressões regulares, Unicode e UTF-16 corretos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ JSON parse/stringify, strings internas UTF-16 com lone surrogates, indexação/iteração/conversões selecionadas e RegExp próprio limitado.
  - Origem: Base JSON/strings/RegExp + incrementos UTF-16 e correções de semântica/fixtures; não completam ECMA-262.
  - Lacunas/limites: RegExp Unicode/UTF-16 integral, matching de surrogates isolados e escapes/flags/backreferences/lookbehind completos; fronteira DOM UTF-8 recusa valores irrepresentáveis.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro.
  - Fontes: [docs/roadmap/implementações Planejadas.md:83](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/json.rs](../../crates/web-javascript/src/json.rs); [crates/web-javascript/src/strings.rs](../../crates/web-javascript/src/strings.rs); [crates/web-javascript/src/regexp.rs](../../crates/web-javascript/src/regexp.rs); [crates/web-javascript/src/regexp_runtime.rs](../../crates/web-javascript/src/regexp_runtime.rs); [docs/releases/2026-11-08-javascript-roadmap-item1.md](../../docs/releases/2026-11-08-javascript-roadmap-item1.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **1.7** Garbage collector, referências fracas, finalização e integração segura com objetos DOM.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ GC mark/sweep não móvel em pontos seguros; roots de closures/classes/Promises/listeners/timers/DOM; WeakRef/WeakMap/WeakSet, ephemeron até ponto fixo e FinalizationRegistry com cleanup posterior.
  - Origem: Base GC/roots + incrementos WeakRef, coleções fracas e FinalizationRegistry de 08/11.
  - Lacunas/limites: Alvos fracos limitados a Object/Array/Function; Host/Native/Symbol recusados; integração Web completa e garantias de encerramento pendentes, sem estabilidade prolongada comprovada.
  - Validação: Ciclo 16: web-javascript 440/440 e bindings 130/130. Testes próprios do subconjunto; não são Test262 nem prova do bullet inteiro.
  - Fontes: [docs/roadmap/implementações Planejadas.md:85](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/gc.rs](../../crates/web-javascript/src/gc.rs); [crates/web-javascript/src/weak_refs.rs](../../crates/web-javascript/src/weak_refs.rs); [crates/web-javascript/src/weak_collections.rs](../../crates/web-javascript/src/weak_collections.rs); [crates/web-javascript/src/finalization_registry.rs](../../crates/web-javascript/src/finalization_registry.rs); [crates/web-bindings/src/events.rs](../../crates/web-bindings/src/events.rs); [docs/releases/2026-11-08-weakref.md](../../docs/releases/2026-11-08-weakref.md); [docs/releases/2026-11-08-weak-collections.md](../../docs/releases/2026-11-08-weak-collections.md); [docs/releases/2026-11-08-finalization-registry.md](../../docs/releases/2026-11-08-finalization-registry.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **1.8** Internacionalização: números, datas, calendários, fusos, ordenação e segmentação de texto.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Intl/ECMA-402: formatação de números/datas, calendários, fusos, ordenação e segmentação por locale. Date/Number comuns e suporte parcial Unicode não satisfazem este item.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:87](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [docs/releases/2026-11-08-javascript-roadmap-item1.md](../../docs/releases/2026-11-08-javascript-roadmap-item1.md); [crates/web-javascript/src/stdlib.rs](../../crates/web-javascript/src/stdlib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 2. WebAssembly

- [ ] **2.1** Validação, instanciação e execução de módulos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ validate/Module/Instance síncronos e execução real via wasmi 2.0 sem JIT, incluindo loops/chamadas/traps, preflight e fuel.
  - Origem: Primeiro recorte Wasm de 08/11.
  - Lacunas/limites: Somente módulos numéricos stateless; validade binária não implica elegibilidade de Instance; plataforma Wasm integral pendente.
  - Validação: Ciclo 16: web-wasm 10/10, VM 440/440 e bindings 130/130. Não certifica Wasm integral nem aplicações-alvo.
  - Fontes: [docs/roadmap/implementações Planejadas.md:97](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-wasm/src/lib.rs](../../crates/web-wasm/src/lib.rs); [crates/web-javascript/src/wasm.rs](../../crates/web-javascript/src/wasm.rs); [docs/releases/2026-11-08-stateless-webassembly.md](../../docs/releases/2026-11-08-stateless-webassembly.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **2.2** Memórias, tabelas, imports, exports e integração com JavaScript.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Exports numéricos i32/i64/f32/f64 com ponte JavaScript; i64↔BigInt exato, identidade e GC; até 32 parâmetros e um resultado.
  - Origem: Primeiro recorte Wasm + segunda rodada i64/BigInt de 08/11.
  - Lacunas/limites: Memórias, tabelas, imports JavaScript, globals mutáveis, start, segmentos, referências e estado persistente recusados; Store efêmero por chamada.
  - Validação: Ciclo 16: web-wasm 10/10, VM 440/440 e bindings 130/130. Não certifica Wasm integral nem aplicações-alvo.
  - Fontes: [docs/roadmap/implementações Planejadas.md:99](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-wasm/src/lib.rs](../../crates/web-wasm/src/lib.rs); [crates/web-javascript/src/wasm.rs](../../crates/web-javascript/src/wasm.rs); [crates/web-bindings/src/wasm_i64_tests.rs](../../crates/web-bindings/src/wasm_i64_tests.rs); [docs/releases/2026-11-08-webassembly-i64-bigint.md](../../docs/releases/2026-11-08-webassembly-i64-bigint.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **2.3** Compilação/carregamento por streaming.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: compileStreaming/instantiateStreaming e carregamento incremental não implementados; BufferSource inteiro e CompilationMode::Eager não são streaming; compile/instantiate Promise também ausentes.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:101](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-wasm/src/lib.rs](../../crates/web-wasm/src/lib.rs); [crates/web-javascript/src/wasm.rs](../../crates/web-javascript/src/wasm.rs); [docs/releases/2026-11-08-stateless-webassembly.md](../../docs/releases/2026-11-08-stateless-webassembly.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **2.4** Extensões estabilizadas exigidas pelas aplicações atendidas.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Backend wasmi com recursos configurados explicitamente e ponte i64/BigInt ampliada.
  - Origem: Configuração do primeiro recorte Wasm + extensão i64/BigInt da segunda rodada.
  - Lacunas/limites: Extensões necessárias às aplicações-alvo ainda não mapeadas/comprovadas; memory64, SIMD, referências, multi-value/multi-memory, tail-call, extended-const, custom-page-size e wide-arithmetic fora da configuração declarada.
  - Validação: Ciclo 16: web-wasm 10/10, VM 440/440 e bindings 130/130. Não certifica Wasm integral nem aplicações-alvo.
  - Fontes: [docs/roadmap/implementações Planejadas.md:103](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-wasm/src/lib.rs](../../crates/web-wasm/src/lib.rs); [crates/web-wasm/Cargo.toml](../../crates/web-wasm/Cargo.toml); [docs/releases/2026-11-08-stateless-webassembly.md](../../docs/releases/2026-11-08-stateless-webassembly.md); [docs/releases/2026-11-08-webassembly-i64-bigint.md](../../docs/releases/2026-11-08-webassembly-i64-bigint.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 3. HTML, documentos e DOM

- [ ] **3.1** Parser HTML completo, incluindo recuperação padronizada de documentos malformados.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Parser HTML próprio com recuperação limitada, raw text/RCDATA/entidades e normalização html/head/body; quotas de 1 MiB, 20.000 nós e profundidade 128.
  - Origem: Parser próprio preexistente + normalização de documentos de 08/11.
  - Lacunas/limites: Tokenização, construção de árvore e recuperação WHATWG completas; tabelas/select/foreign contexts completos.
  - Validação: Ciclo 16: web-dom 3/3, web-html 9/9, web-css 36/36 e bindings 130/130. Sem WPT/conformidade geral.
  - Fontes: [docs/roadmap/implementações Planejadas.md:113](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-html/src/lib.rs](../../crates/web-html/src/lib.rs); [docs/releases/2026-11-08-normalized-html-documents.md](../../docs/releases/2026-11-08-normalized-html-documents.md); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.2** Construção correta da árvore, modos de documento e compatibilidade legada.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Wrappers únicos; head/body/documentElement reais; incorporação limitada de atributos duplicados e DocumentMode/compatMode.
  - Origem: Base de DOM/doctype + normalização/getters estruturais de 08/11.
  - Lacunas/limites: Modos de inserção completos, nós Comment/DocumentType públicos e compatibilidade/layout quirks integral.
  - Validação: Ciclo 16: web-dom 3/3, web-html 9/9, web-css 36/36 e bindings 130/130. Sem WPT/conformidade geral.
  - Fontes: [docs/roadmap/implementações Planejadas.md:115](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-html/src/lib.rs](../../crates/web-html/src/lib.rs); [crates/web-dom/src/lib.rs](../../crates/web-dom/src/lib.rs); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.3** Elementos HTML, atributos, propriedades refletidas e comportamento de cada elemento.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Elementos/atributos e getters/setters selecionados: title, textContent, value, checked/disabled, style/classList; controles input/textarea/select/button; dimensões naturais de imagem e audio.duration limitados.
  - Origem: Base de DOM/controles + getters/título, dimensões naturais e áudio limitado de 08/11.
  - Lacunas/limites: Reflexão, estados e algoritmos de todos os elementos; getter innerHTML e mídia completa ausentes.
  - Validação: Ciclo 16: web-dom 3/3, web-html 9/9, web-css 36/36 e bindings 130/130. Sem WPT/conformidade geral.
  - Fontes: [docs/roadmap/implementações Planejadas.md:117](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-bindings/src/media.rs](../../crates/web-bindings/src/media.rs); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.4** Namespaces, SVG integrado ao documento e MathML.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Namespaces, algoritmos foreign-content, SVG integrado ao DOM/estilos/interação e MathML. SVG estático decodificado como imagem é uma capacidade relacionada, não implementação deste subitem.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:119](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-dom/src/lib.rs](../../crates/web-dom/src/lib.rs); [crates/web-html/src/lib.rs](../../crates/web-html/src/lib.rs); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md); [docs/specifications/images-subset.md](../../docs/specifications/images-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.5** Criação, movimentação, clonagem, adoção e remoção de nós.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ createElement/createTextNode, appendChild/insertBefore/removeChild/replaceChild/remove/cloneNode com preflight e limites em um documento.
  - Origem: Base de mutações/clone do DOM + integração ao documento normalizado de 08/11.
  - Lacunas/limites: importNode/adoptNode, adoção entre documentos, DocumentFragment público e algoritmos/protótipos DOM completos.
  - Validação: Ciclo 16: web-dom 3/3, web-html 9/9, web-css 36/36 e bindings 130/130. Sem WPT/conformidade geral.
  - Fontes: [docs/roadmap/implementações Planejadas.md:121](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.6** Fragmentos, templates, coleções vivas, consultas e seletores.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Fragmentos contextuais no setter innerHTML; templates/noscript inativos; querySelector/querySelectorAll/matches/closest com parser CSS compartilhado, ordem e deduplicação; lista snapshot com length/índices consumível por Array.from.
  - Origem: Base de consultas/mutações + fragmentos contextuais; segunda rodada seletores DOM/CSS e Array.from de 08/11.
  - Lacunas/limites: template.content/DocumentFragment, coleções vivas/iteradores DOM, :scope/:has/namespaces/pseudoelementos/Shadow DOM, contextos table/select/foreign e getter innerHTML.
  - Validação: Ciclo 16: web-dom 3/3, web-html 9/9, web-css 36/36 e bindings 130/130. Sem WPT/conformidade geral.
  - Fontes: [docs/roadmap/implementações Planejadas.md:123](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-html/src/lib.rs](../../crates/web-html/src/lib.rs); [crates/web-css/src/selector_query.rs](../../crates/web-css/src/selector_query.rs); [crates/web-bindings/src/selector_query_tests.rs](../../crates/web-bindings/src/selector_query_tests.rs); [crates/web-bindings/src/array_from_tests.rs](../../crates/web-bindings/src/array_from_tests.rs); [docs/releases/2026-11-08-shared-dom-selectors.md](../../docs/releases/2026-11-08-shared-dom-selectors.md); [docs/releases/2026-11-08-array-from-dom.md](../../docs/releases/2026-11-08-array-from-dom.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.7** Web Components: custom elements, Shadow DOM, slots e ciclos de vida.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: customElements, ShadowRoot/attachShadow, slots/distribuição e callbacks de ciclo de vida. Templates inativos não equivalem a Web Components.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:125](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [crates/web-dom/src/lib.rs](../../crates/web-dom/src/lib.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.8** Observadores de mutação, tamanho e interseção.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: MutationObserver, ResizeObserver e IntersectionObserver, incluindo checkpoints e entrega. Contador de mutações/cache e medidas síncronas não oferecem essas APIs.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:127](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md); [crates/web-bindings/src/geometry.rs](../../crates/web-bindings/src/geometry.rs); [crates/web-renderer/src/geometry.rs](../../crates/web-renderer/src/geometry.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **3.9** Interfaces e conversões Web IDL, incluindo protótipos e descritores corretos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Ponte Host com verificação de receiver, conversões/dicionários observáveis de Event/CustomEvent, valores DOM/erros limitados e construtores host opt-in.
  - Origem: Ponte Host preexistente + conversões/construtores Event/CustomEvent de 08/11.
  - Lacunas/limites: Web IDL, DOMException, protótipos/descritores/instanceof/subclasses e interfaces completos.
  - Validação: Ciclo 16: web-dom 3/3, web-html 9/9, web-css 36/36 e bindings 130/130. Sem WPT/conformidade geral.
  - Fontes: [docs/roadmap/implementações Planejadas.md:129](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/events.rs](../../crates/web-bindings/src/events.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-javascript/src/host_constructors.rs](../../crates/web-javascript/src/host_constructors.rs); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 4. Event loop e ciclo de vida

- [ ] **4.1** Tarefas, microtasks e checkpoints com ordenação correta.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Jobs Promise/queueMicrotask FIFO e checkpoints após scripts, módulos e callbacks; timers em lotes limitados; dispatchEvent síncrono sem checkpoint adicional entre listeners.
  - Origem: Base de jobs/timers + checkpoints/ciclo de vida de 08/11.
  - Lacunas/limites: Fontes/prioridades de tarefas Web, checkpoints de todas as fases, coordenação/rejection tracking e event loop integral.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, IPC 23/23, process 18/18 e subprocess 12/12. Não prova vsync ou todos os eventos Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:139](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/promises.rs](../../crates/web-javascript/src/promises.rs); [crates/web-javascript/src/finalization_registry.rs](../../crates/web-javascript/src/finalization_registry.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-bindings/src/lifecycle.rs](../../crates/web-bindings/src/lifecycle.rs); [crates/web-bindings/src/events.rs](../../crates/web-bindings/src/events.rs); [docs/releases/2026-11-08-document-lifecycle.md](../../docs/releases/2026-11-08-document-lifecycle.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **4.2** Timers, frames de animação e sincronização com renderização.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Timers limitados e requestAnimationFrame/cancelAnimationFrame; snapshot de até 32 callbacks, timestamp real do host, validação monotônica IPC v8 e callbacks antes da display list; oferta à aba exibida elegível.
  - Origem: Timers preexistentes + segunda rodada rAF/cancelAnimationFrame e IPC v8 de 08/11.
  - Lacunas/limites: Vsync, apresentação GPU, frequência e sincronização Web completas; timers vencidos podem adiar frames.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, IPC 23/23, process 18/18 e subprocess 12/12. Não prova vsync ou todos os eventos Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:141](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/animation.rs](../../crates/web-bindings/src/animation.rs); [crates/web-bindings/src/animation_tests.rs](../../crates/web-bindings/src/animation_tests.rs); [crates/web-renderer/src/lib.rs](../../crates/web-renderer/src/lib.rs); [crates/browser-ipc/src/lib.rs](../../crates/browser-ipc/src/lib.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-animation-frames.md](../../docs/releases/2026-11-08-animation-frames.md); [docs/adr/0021-host-animation-frame-pulses.md](../../docs/adr/0021-host-animation-frame-pulses.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **4.3** Scripts clássicos, módulos, `async`, `defer` e scripts inseridos dinamicamente.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Scripts clássicos e módulos do lote autorizado; fila DOM única de externos defer e módulos sem async; inline ignora async/defer.
  - Origem: Scripts preexistentes + módulos e fila defer/ciclo de vida de 08/11.
  - Lacunas/limites: Externos async/módulos async recusados; parser incremental/bloqueante real, execução de scripts inseridos dinamicamente e top-level await ausentes.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, IPC 23/23, process 18/18 e subprocess 12/12. Não prova vsync ou todos os eventos Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:143](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lifecycle.rs](../../crates/web-bindings/src/lifecycle.rs); [docs/releases/2026-11-08-document-lifecycle.md](../../docs/releases/2026-11-08-document-lifecycle.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **4.4** Estados de carregamento, eventos de sucesso/falha e cancelamento.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ readyState loading/interactive/complete, readystatechange, DOMContentLoaded/load e inicialização única; erros comuns reportados e Resource com rollback/desativação.
  - Origem: Incremento de readyState/readystatechange/ciclo inicial de 08/11 sobre sessão preexistente.
  - Lacunas/limites: Estados/eventos de sucesso e falha de todas as categorias de recurso, cancelamento padronizado e lifecycle Web completo; complete/load não provam carga bem-sucedida.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, IPC 23/23, process 18/18 e subprocess 12/12. Não prova vsync ou todos os eventos Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:145](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lifecycle.rs](../../crates/web-bindings/src/lifecycle.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/releases/2026-11-08-document-lifecycle.md](../../docs/releases/2026-11-08-document-lifecycle.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **4.5** Visibilidade, congelamento, retomada e descarte de páginas.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Shell considera aba ativa e janela oculta ao ofertar rAF e evita replay de pulsos de abas inativas.
  - Origem: Estado nativo de abas/janela preexistente + elegibilidade dos pulsos rAF de 08/11.
  - Lacunas/limites: Page Visibility API, freeze/resume/discard e estados/eventos padronizados; ocultação observada pelo host não equivale à API Web.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, IPC 23/23, process 18/18 e subprocess 12/12. Não prova vsync ou todos os eventos Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:147](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-animation-frames.md](../../docs/releases/2026-11-08-animation-frames.md); [docs/releases/2026-11-08-document-lifecycle.md](../../docs/releases/2026-11-08-document-lifecycle.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **4.6** Coordenação entre documentos, workers e processos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ DocumentSession retida no renderer, supervisor/IPC transacionais e coordenação host/processo para dispatch, timers e frames.
  - Origem: Sessões/processos preexistentes + integração de lifecycle/frames de 08/11.
  - Lacunas/limites: Coordenação Web de event loops múltiplos, documentos aninhados e workers; workers ausentes.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, IPC 23/23, process 18/18 e subprocess 12/12. Não prova vsync ou todos os eventos Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:149](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [crates/web-renderer/src/lib.rs](../../crates/web-renderer/src/lib.rs); [crates/web-bindings/src/lifecycle.rs](../../crates/web-bindings/src/lifecycle.rs); [docs/releases/2026-11-08-document-lifecycle.md](../../docs/releases/2026-11-08-document-lifecycle.md); [docs/releases/2026-11-08-animation-frames.md](../../docs/releases/2026-11-08-animation-frames.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 5. Eventos, entrada e edição

- [ ] **5.1** Captura, alvo, propagação, cancelamento e ações padrão.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Captura/alvo/bubble, caminho congelado, stopPropagation/stopImmediatePropagation, once/passive/remoção, cancelamento, nós destacados e dispatch síncrono; ações nativas limitadas de cliques/formulários na shell.
  - Origem: Base de listeners/fases + Event/CustomEvent/dispatchEvent de 08/11.
  - Lacunas/limites: Ações padrão Web completas e integradas ao cancelamento; dispatchEvent programático não dispara navegação, envio ou foco.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15 e desktop unitário 35/35. Entrada/edição Web completa e avaliação interativa ampla não verificadas.
  - Fontes: [docs/roadmap/implementações Planejadas.md:155](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/events.rs](../../crates/web-bindings/src/events.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md); [docs/releases/2026-11-08-programmatic-events.md](../../docs/releases/2026-11-08-programmatic-events.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **5.2** Event, CustomEvent, EventTarget e eventos especializados.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Construtores Event/CustomEvent host, detail com identidade/GC, dispatchEvent em Window/Document/Node e add/removeEventListener.
  - Origem: Base de listeners Host + construtores e dispatchEvent de 08/11.
  - Lacunas/limites: EventTarget independente, eventos especializados, handleEvent e protótipos/instanceof/subclasses/descritores Web IDL completos.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15 e desktop unitário 35/35. Entrada/edição Web completa e avaliação interativa ampla não verificadas.
  - Fontes: [docs/roadmap/implementações Planejadas.md:157](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/events.rs](../../crates/web-bindings/src/events.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-javascript/src/host_constructors.rs](../../crates/web-javascript/src/host_constructors.rs); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **5.3** Teclado, mouse, ponteiro, toque, wheel, gestos e composição de texto.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Controles egui encaminham Input/Change/Click à sessão; Enter em inputs aciona envio limitado.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: KeyboardEvent/MouseEvent/PointerEvent/TouchEvent/WheelEvent completos, dados de dispositivo, gestos e composição/IME Web. Tabela de nomes reconhecidos não comprova integração desses dispositivos.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15 e desktop unitário 35/35. Entrada/edição Web completa e avaliação interativa ampla não verificadas.
  - Fontes: [docs/roadmap/implementações Planejadas.md:159](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-renderer/src/lib.rs](../../crates/web-renderer/src/lib.rs); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **5.4** Foco, seleção, ranges, edição e `contenteditable`.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Foco, seleção e edição nos TextEdit nativos; atualização value/input/change e foco visível dos links pela Response egui.
  - Origem: Controles/edição nativos preexistentes + contorno de foco nos links de 08/11.
  - Lacunas/limites: Foco DOM/tabindex/ordem Tab completa, Selection/Range, contenteditable e eventos DOM de foco completos.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15 e desktop unitário 35/35. Entrada/edição Web completa e avaliação interativa ampla não verificadas.
  - Fontes: [docs/roadmap/implementações Planejadas.md:161](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-web-link-keyboard-focus.md](../../docs/releases/2026-11-08-web-link-keyboard-focus.md); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **5.5** Clipboard, drag-and-drop e integração com métodos de entrada do sistema.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Clipboard API/eventos, drag-and-drop/DataTransfer e integração de métodos de entrada/composição ao conteúdo Web não demonstrados. TextEdit host não basta; copiar/colar ou IME não foram verificados nesta auditoria.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:163](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **5.6** Caminhos compostos de eventos em Shadow DOM.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: composedPath, retargeting e caminhos compostos em Shadow DOM; composed truthy é recusado na construção. Propagação comum não satisfaz este item.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:165](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/events.rs](../../crates/web-bindings/src/events.rs); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md); [docs/specifications/html-dom-subset.md](../../docs/specifications/html-dom-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **5.7** AbortController/AbortSignal integrados às operações canceláveis.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: AbortController/AbortSignal e integração com listeners/operações canceláveis. signal não nullish é recusado; clearTimeout/cancelAnimationFrame e transações do host não implementam essas APIs.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:167](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/specifications/events-subset.md](../../docs/specifications/events-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 6. CSS e cascata

- [ ] **6.1** Parsing, seletores, especificidade, herança e cascata completos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Parser/cascata próprios, herança selecionada e seletores compostos/listas/combinadores/atributos; especificidade lexicográfica, todos os IDs do composto, inline e !important.
  - Origem: Parser/matcher/cascata próprios preexistentes + correção de especificidade e compartilhamento dos seletores de 08/11.
  - Lacunas/limites: Parsing, seletores, cascata/herança, origens e recuperação CSS completos; :has, estados dinâmicos e pseudoelementos pendentes.
  - Validação: Ciclo 16: web-css 36/36 e bindings 130/130. Não é suíte CSS/WPT completa nem certificação visual.
  - Fontes: [docs/roadmap/implementações Planejadas.md:173](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [crates/web-css/src/selector_query.rs](../../crates/web-css/src/selector_query.rs); [docs/releases/2026-11-08-css-specificity.md](../../docs/releases/2026-11-08-css-specificity.md); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.2** Camadas, escopos, variáveis, funções, unidades e valores computados.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Custom properties herdadas em mapa, var() com fallback/limites, unidades px/em/rem/%/vw/vh/vmin/vmax/físicas, valores Style computados e repeat() de Grid limitados.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: @layer/@scope, variáveis e CSS-wide keywords completos, calc/funções/unidades e valores computados gerais.
  - Validação: Ciclo 16: web-css 36/36 e bindings 130/130. Não é suíte CSS/WPT completa nem certificação visual.
  - Fontes: [docs/roadmap/implementações Planejadas.md:175](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/releases/2026-11-08-css-specificity.md](../../docs/releases/2026-11-08-css-specificity.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.3** Pseudoclasses, pseudo-elementos e conteúdo gerado.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ :not/:is/:where, nth/first/last/only/of-type, :empty/:link/:checked/:disabled/:enabled/:lang no subconjunto.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: hover/focus e demais pseudoclasses, pseudoelementos e conteúdo gerado.
  - Validação: Ciclo 16: web-css 36/36 e bindings 130/130. Não é suíte CSS/WPT completa nem certificação visual.
  - Fontes: [docs/roadmap/implementações Planejadas.md:177](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [crates/web-css/src/selector_query.rs](../../crates/web-css/src/selector_query.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.4** Media queries, feature queries e container queries.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ @media screen/all com dimensões/orientation/and/listas e reavaliação de viewport; @supports declarações/selector/not/and/or conservadores.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Container queries, media features/ranges/preferências e sintaxe completa.
  - Validação: Ciclo 16: web-css 36/36 e bindings 130/130. Não é suíte CSS/WPT completa nem certificação visual.
  - Fontes: [docs/roadmap/implementações Planejadas.md:179](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/releases/2026-11-07-engine-v09.md](../../docs/releases/2026-11-07-engine-v09.md); [fixtures/css-conditional-rules.html](../../fixtures/css-conditional-rules.html).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.5** Fontes, cores, fundos, bordas, sombras, máscaras e filtros.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Tamanho/peso/line-height de fonte, cores simples, fundo sólido/alpha, borda uniforme e shorthands limitados.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: @font-face/famílias reais, imagens de fundo completas, bordas avançadas/radius, sombras, máscaras e filtros.
  - Validação: Ciclo 16: web-css 36/36 e bindings 130/130. Não é suíte CSS/WPT completa nem certificação visual.
  - Fontes: [docs/roadmap/implementações Planejadas.md:181](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.6** Transforms, transições, animações e view transitions.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Transforms CSS, transitions, @keyframes/animations e view transitions. Animação da shell, rAF e zoom do host são capacidades distintas.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:183](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/releases/2026-11-08-css-specificity.md](../../docs/releases/2026-11-08-css-specificity.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.7** Propriedades lógicas, escrita vertical e direção de texto.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Propriedades lógicas, writing-mode vertical e direção/bidi corretos. Tratar text-align start/end como alinhamento existente não implementa direção.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:185](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **6.8** CSSOM, folhas dinâmicas e consulta de estilos computados.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Propriedades inline limitadas em style; mutações de atributos/style/nós style repercutem no recálculo, com folhas autorizadas relidas.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: CSSOM/CSSStyleSheet/CSSRule/insertRule/deleteRule/adoptedStyleSheets, folhas dinâmicas completas e getComputedStyle.
  - Validação: Ciclo 16: web-css 36/36 e bindings 130/130. Não é suíte CSS/WPT completa nem certificação visual.
  - Fontes: [docs/roadmap/implementações Planejadas.md:187](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [crates/web-renderer/src/lib.rs](../../crates/web-renderer/src/lib.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [docs/releases/2026-11-08-css-specificity.md](../../docs/releases/2026-11-08-css-specificity.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 7. Layout e geometria

- [ ] **7.1** Fluxos block/inline e formação de linhas.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Fluxos block/inline/inline-block, formação de linhas, alinhamento e whitespace normal/nowrap/pre/pre-wrap/pre-line.
  - Origem: Fluxos preexistentes + normalização de whitespace/pre-line de 08/11.
  - Lacunas/limites: Inline formatting e layout de linhas completos, fragmentação, quebra Unicode/graphemes e métricas entre runs complexos.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:197](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.2** Dimensionamento intrínseco, margens, padding, bordas e restrições de tamanho.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ width/height/min-width/min-height/max-width, padding/margens/borda/box-sizing e dimensões naturais de imagens; alturas percentuais com referência definida.
  - Origem: Box model preexistente + dimensões naturais de imagem e recorte Flex de 08/11.
  - Lacunas/limites: min-content/max-content completos, agregação intrínseca entre irmãos, todas as restrições/margin collapsing e replaced elements gerais.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:199](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md); [docs/releases/2026-11-08-flex-cross-axis.md](../../docs/releases/2026-11-08-flex-cross-axis.md); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.3** Flexbox, Grid, tabelas, floats, colunas e fragmentação.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Flex row/column/reverse/wrap/grow/shrink/basis/gap e alinhamento transversal row-nowrap com altura definida; Grid de colunas explícitas/repeat e tabelas HTML simples.
  - Origem: Flex/Grid/tabelas preexistentes + alinhamento transversal row-nowrap de 08/11.
  - Lacunas/limites: Flex completo, baseline/order/align-self/wrap-reverse; Grid placement/spans/minmax/subgrid; rowspan/colspan, floats, multicolunas e fragmentação.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:201](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [docs/releases/2026-11-08-flex-cross-axis.md](../../docs/releases/2026-11-08-flex-cross-axis.md); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.4** Posicionamento static/relative/absolute/fixed/sticky.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ static, relative e absolute com offsets e containing block limitado.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: fixed/sticky, static-position completo, esticamento vertical top/bottom e casos inline/fragmentados.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:203](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [fixtures/positioned-layout.html](../../fixtures/positioned-layout.html).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.5** Containing blocks, stacking contexts, `z-index` e ordem de pintura.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Absolute usa padding box do ancestral posicionado/viewport; pintura/translações seguem árvore e display list.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Stacking contexts, z-index e ordem normativa completa de pintura; containing block de inline relativo usa aproximação envolvente.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:205](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.6** Overflow, scroll containers, scroll snap e ancoragem de rolagem.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Overflow visível contribui para extensão rolável; hidden/clip retangular aninhado em layout/IPC/links/controles/CPU/GPU; rolagem da página e offset no histórico nativo.
  - Origem: Overflow/clips/rolagem preexistentes + scroll no histórico de 08/11.
  - Lacunas/limites: Scroll containers CSS, overflow-x/y/auto/scroll, scroll snap e ancoragem.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:207](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/releases/2026-11-08-history-scroll.md](../../docs/releases/2026-11-08-history-scroll.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.7** Geometria correta para zoom, DPI e transforms.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Coordenadas CSS separadas de zoom/pixels_per_point; viewport dividido pelo zoom e cena pintada com escala física, clipping e retângulos de interação.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: CSS transforms e geometria normativa completa; correção visual em todos os DPI/zooms não comprovada pelo baseline.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:209](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.8** `getBoundingClientRect`, medidas client/offset/scroll e hit testing.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ clientWidth/clientHeight/clientLeft/clientTop calculados pelo layout e innerWidth/innerHeight reais; hit areas de links/controles recortadas.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: getBoundingClientRect, medidas offset/scroll, hit testing DOM completo/elementFromPoint; serviço atual limitado a 64 resoluções síncronas por sessão.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:211](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/geometry.rs](../../crates/web-bindings/src/geometry.rs); [crates/web-renderer/src/geometry.rs](../../crates/web-renderer/src/geometry.rs); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [docs/adr/0013-events-callable-bridge-and-layout-measurements.md](../../docs/adr/0013-events-callable-bridge-and-layout-measurements.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **7.9** Invalidação e recálculo incremental quando DOM ou estilos mudarem.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Cache de geometria invalidado por mutação/viewport; RenderSession retém DOM/VM e recalcula estilo/layout após eventos/timers/frames.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Invalidação e recálculo incremental por regiões/subárvores/propriedades; o caminho atual recomputa documento/estilos/layout.
  - Validação: Ciclo 16: web-layout 26/26, renderer 15/15 e desktop unitário 35/35. Layout Web/visual/DPI completos não validados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:213](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/geometry.rs](../../crates/web-bindings/src/geometry.rs); [crates/web-renderer/src/geometry.rs](../../crates/web-renderer/src/geometry.rs); [crates/web-renderer/src/lib.rs](../../crates/web-renderer/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 8. Texto e fontes

- [ ] **8.1** Carregamento e seleção de fontes locais e externas.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Seleção/carregamento de fontes da página: fontes locais do SO, font-family/font matching, @font-face/FontFace e fontes externas autorizadas. Ubuntu Light fixa embutida é base relacionada, não a implementação solicitada.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:219](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **8.2** Shaping, kerning, ligaturas, fontes variáveis e fallback.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Métricas por glifo e horizontal_kern de fontdue, negrito sintético, atlas e rasterização.
  - Origem: Fontdue/kerning preexistentes + separação métricas/materialização e culling de glifos de 08/11.
  - Lacunas/limites: Shaping, ligaturas, variações OpenType e fallback/font matching; kerning não equivale a shaping completo.
  - Validação: Ciclo 16: layout 26/26, graphics 9/9 e paint 7/7. Shaping/bidi/fallback e consistência tipográfica geral não certificados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:221](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **8.3** Unicode, escrita bidirecional e diferentes sistemas de escrita.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Texto UTF-8 no DOM, Unicode preservado em runs e espaços U+00A0/U+202F/U+2003 não colapsados; strings UTF-16 na VM.
  - Origem: Unicode preexistente + preservação explícita dos espaços no recorte de whitespace de 08/11.
  - Lacunas/limites: Bidi, shaping por sistemas de escrita, graphemes e quebra Unicode; processamento escalar e fonte fixa não garantem composição nem disponibilidade de glifos.
  - Validação: Ciclo 16: layout 26/26, graphics 9/9 e paint 7/7. Shaping/bidi/fallback e consistência tipográfica geral não certificados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:223](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-javascript/src/strings.rs](../../crates/web-javascript/src/strings.rs); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **8.4** Quebra de linha, hifenização, justificação e decoração.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Wrap/nowrap, preservação/colapso de whitespace, normalização CRLF/CR por nó, altura de linhas vazias e alinhamento básico.
  - Origem: Texto/linhas preexistentes + whitespace normalizado compartilhado de 08/11.
  - Lacunas/limites: Hifenização, justificação, decoração tipográfica completa, tab stops e Unicode line breaking; justify-content Flex não é justificação de texto.
  - Validação: Ciclo 16: layout 26/26, graphics 9/9 e paint 7/7. Shaping/bidi/fallback e consistência tipográfica geral não certificados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:225](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-css/src/lib.rs](../../crates/web-css/src/lib.rs); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md); [docs/specifications/layout-subset.md](../../docs/specifications/layout-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **8.5** Emojis, símbolos e fontes coloridas.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Emojis, sequências ZWJ/variantes, fontes coloridas e fallback de símbolos não demonstrados. O caminho escalar monocromático não comprova cobertura, sem implicar que todo símbolo falha.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:227](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **8.6** Consistência entre medidas, seleção, hit testing e pintura.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Fluxo/largura preferida compartilham normalização por nó; pintores usam métricas/kerning; links/controles seguem deslocamento/clipping do layout e espaço colapsado não ganha área clicável.
  - Origem: Layout/medidas/pintores preexistentes + whitespace compartilhado e glifos recortados de 08/11.
  - Lacunas/limites: Selection/Range e hit testing textual/graphemes, equivalência entre nós fragmentados, fallback e consistência visual ponta a ponta; fonte dos controles egui segue caminho separado.
  - Validação: Ciclo 16: layout 26/26, graphics 9/9 e paint 7/7. Shaping/bidi/fallback e consistência tipográfica geral não certificados.
  - Fontes: [docs/roadmap/implementações Planejadas.md:229](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [docs/releases/2026-11-08-text-whitespace.md](../../docs/releases/2026-11-08-text-whitespace.md); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 9. Rede e carregamento de recursos

- [ ] **9.1** URLs, resolução relativa, codificações e identificação de conteúdo.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ url::Url e resolução relativa, decoding encoding_rs e BOM; parser limitado de MIME/charset com comparação exata de tipos textuais.
  - Origem: URLs/encoding/broker preexistentes + parser Content-Type/MIME/charset de 08/11.
  - Lacunas/limites: Sniffing, recuperação MIME/HTML-meta/@charset e conformidade completa.
  - Validação: Ciclo 16: browser-core 34/34. Testes locais não certificam todos os protocolos, APIs e sites reais.
  - Fontes: [docs/roadmap/implementações Planejadas.md:235](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/lib.rs](../../crates/browser-core/src/lib.rs); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/src/http_metadata.rs](../../crates/browser-core/src/http_metadata.rs); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.2** DNS, conexões, TLS, certificados e protocolos HTTP compatíveis.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ DNS do sistema com prazo, validação e pinning de IP público; reqwest/rustls e HTTP(S) nas portas 80/443 com deadlines.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Cobertura de protocolos, gestão avançada de certificados e compatibilidade geral de transporte não demonstradas.
  - Validação: Ciclo 16: browser-core 34/34. Testes locais não certificam todos os protocolos, APIs e sites reais.
  - Fontes: [docs/roadmap/implementações Planejadas.md:237](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/Cargo.toml](../../crates/browser-core/Cargo.toml); [docs/security/threat-model.md](../../docs/security/threat-model.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.3** Redirects, autenticação, cookies e credenciais.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Até cinco redirects revalidados, sem downgrade HTTPS; URLs com credenciais recusadas.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Autenticação, cookies e credenciais Web não implementados; recusar credenciais não equivale a suportá-las.
  - Validação: Ciclo 16: browser-core 34/34. Testes locais não certificam todos os protocolos, APIs e sites reais.
  - Fontes: [docs/roadmap/implementações Planejadas.md:239](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.4** Cache, revalidação, compressão e reutilização de conexões.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Cache e revalidação explicitamente ausentes; compressão e pool/reuso de conexões no escopo solicitado não verificados. Client criado por destino/requisição não comprova essas capacidades.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:241](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/Cargo.toml](../../crates/browser-core/Cargo.toml); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.5** Fetch, XMLHttpRequest, requests, responses, headers e streams.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Fetch, XMLHttpRequest, objetos Request/Response/Headers e streams expostos à página. Fetch privado em Rust no broker não é a API Fetch Web; bindings verificam ausência de fetch em window.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:243](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.6** Recursos dinâmicos, prioridades, carregamento progressivo e cancelamento.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Recursos dinâmicos, scheduler de prioridades, carga/apresentação progressiva e cancelamento de recursos não verificados. Loader estático, deadlines e descarte de resposta obsoleta são bases relacionadas, não essas capacidades.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:245](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.7** WebSocket, eventos enviados pelo servidor e transportes adicionais conforme o escopo.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: WebSocket/EventSource e demais transportes exigidos pelas aplicações-alvo não verificados na API de página; definir e comprovar o subconjunto escolhido.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:247](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **9.8** Políticas de origem, CORS, conteúdo misto e integridade de recursos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Módulos de mesma origem; CSS/imagens/scripts clássicos públicos de CDNs autorizados, DNS pinado e bloqueio de conteúdo misto/downgrade.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: SOP/CORS/CSP/nosniff completos e integridade de recursos não comprovados; a política atual não exige mesma origem para todo subrecurso.
  - Validação: Ciclo 16: browser-core 34/34. Testes locais não certificam todos os protocolos, APIs e sites reais.
  - Fontes: [docs/roadmap/implementações Planejadas.md:249](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/src/module_assets.rs](../../crates/browser-core/src/module_assets.rs); [docs/adr/0008-authorized-subresources.md](../../docs/adr/0008-authorized-subresources.md); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 10. Navegação e contextos de páginas

- [ ] **10.1** Location, History API, fragmentos e navegação entre documentos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Location real somente leitura e seus componentes; navegação nativa entre documentos e voltar/avançar.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: assign/replace/reload/escrita de Location, History API JavaScript e navegação de fragmentos.
  - Validação: Ciclo 16: desktop unitário 35/35, process 18/18 e subprocess 12/12. Resultados não ampliam o contrato de navegação/Histórico Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:259](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/browser-core/src/lib.rs](../../crates/browser-core/src/lib.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-history-scroll.md](../../docs/releases/2026-11-08-history-scroll.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **10.2** Histórico de sessão e restauração de estado.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Histórico nativo com URL e scroll CSS 2D por entrada, até 200 por aba; commit válido e restauração única com clamp.
  - Origem: Histórico nativo preexistente + offsets CSS 2D e restauração de 08/11.
  - Lacunas/limites: BFCache, DOM/heap/formulários e persistência do histórico/offsets após reinício.
  - Validação: Ciclo 16: desktop unitário 35/35, process 18/18 e subprocess 12/12. Resultados não ampliam o contrato de navegação/Histórico Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:261](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-history-scroll.md](../../docs/releases/2026-11-08-history-scroll.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **10.3** Iframes, popups, contextos aninhados e comunicação entre documentos.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Iframes, popups, contextos Web aninhados e comunicação entre documentos. Abas nativas independentes e sessão de documento único não atendem ao escopo.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:263](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-history-scroll.md](../../docs/releases/2026-11-08-history-scroll.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **10.4** `postMessage`, MessageChannel e structured clone.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: postMessage, MessageChannel e structured clone Web não verificados nos globals/bindings; JSON interno do IPC não é structured clone.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:265](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/browser-ipc/src/lib.rs](../../crates/browser-ipc/src/lib.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **10.5** Formulários GET/POST, multipart, upload e validação.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ GET com encoding Unicode, successful controls, submit padrão/overrides e controles select/checkbox sob limites.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: POST explicitamente recusado; multipart, upload e validação HTML completa pendentes.
  - Validação: Ciclo 16: desktop unitário 35/35, process 18/18 e subprocess 12/12. Resultados não ampliam o contrato de navegação/Histórico Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:267](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/validation/2026-11-03-controls-v051.md](../../docs/validation/2026-11-03-controls-v051.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **10.6** Downloads, redirecionamentos e protocolos externos autorizados.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Redirects HTTP limitados e revalidados pelo broker.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Downloads/gestor de arquivos recebidos e despacho de protocolos externos autorizados não verificados; esquemas não admitidos são recusados.
  - Validação: Ciclo 16: desktop unitário 35/35, process 18/18 e subprocess 12/12. Resultados não ampliam o contrato de navegação/Histórico Web.
  - Fontes: [docs/roadmap/implementações Planejadas.md:269](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/src/lib.rs](../../crates/browser-core/src/lib.rs); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md); [docs/releases/2026-11-08-history-scroll.md](../../docs/releases/2026-11-08-history-scroll.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [x] **10.7** Navegação transacional: respostas antigas nunca substituem a navegação atual.
  - **Estado: Implementado.**
  - Implementação concreta: ✅ No pipeline existente, pending/queued e identidade de documento/sessão/geração rejeitam respostas obsoletas antes do commit; storage só confirma checkpoint válido.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Nenhuma lacuna de implementação identificada no contrato delimitado. Essa classificação não certifica todas as condições concorrentes nem compatibilidade geral; a validação funcional continua separada.
  - Validação: Ciclo 16: desktop unitário 35/35, process 18/18 e subprocess 12/12. Resultados não ampliam o contrato de navegação/Histórico Web.
  - Testes exatos aprovados no ciclo 16: shell::tests::queued_navigation_discards_obsolete_result; shell::tests::history_commits_only_successful_navigation; shell::tests::only_exact_committed_operations_can_preserve_queued_checkpoints.
  - Fontes: [docs/roadmap/implementações Planejadas.md:271](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 11. Persistência, workers e aplicações offline

- [ ] **11.1** Cookies, localStorage e sessionStorage.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ sessionStorage real, ordenado, em memória e separado por aba/origem HTTP(S), com seed/snapshot validados.
  - Origem: Incremento sessionStorage de 08/11.
  - Lacunas/limites: Cookies, localStorage, eventos storage e interfaces Storage/Web IDL completas.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, process 18/18 e IPC 23/23. Cobertura de sessionStorage não certifica as demais APIs de persistência.
  - Fontes: [docs/roadmap/implementações Planejadas.md:277](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/storage.rs](../../crates/web-bindings/src/storage.rs); [crates/browser-process/src/session_storage.rs](../../crates/browser-process/src/session_storage.rs); [docs/specifications/session-storage-subset.md](../../docs/specifications/session-storage-subset.md); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **11.2** IndexedDB, Cache Storage e quotas.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: IndexedDB e Cache Storage não implementados. As quotas existentes de sessionStorage (128 pares/64 KiB por origem; 16 origens/aba) não implementam as APIs nomeadas.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:279](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md); [crates/browser-process/src/session_storage.rs](../../crates/browser-process/src/session_storage.rs); [crates/browser-ipc/src/lib.rs](../../crates/browser-ipc/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **11.3** Particionamento por origem e políticas de descarte.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Buckets da origem final HTTP(S) separados por aba; fechamento/reinício descartam dados; sem bucket opaco/null e sem eviction silenciosa ao esgotar quotas.
  - Origem: Particionamento/quotas do incremento sessionStorage de 08/11.
  - Lacunas/limites: Particionamento e políticas de descarte amplos para os demais armazenamentos e contextos.
  - Validação: Ciclo 16: bindings 130/130, renderer 15/15, process 18/18 e IPC 23/23. Cobertura de sessionStorage não certifica as demais APIs de persistência.
  - Fontes: [docs/roadmap/implementações Planejadas.md:281](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-process/src/session_storage.rs](../../crates/browser-process/src/session_storage.rs); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **11.4** Dedicated Workers, Shared Workers e Service Workers.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Dedicated Workers, Shared Workers e Service Workers Web. Processos de renderer/IA são infraestrutura interna, não essas APIs.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:283](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **11.5** Atualização, ativação e encerramento correto desses workers.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Atualização, ativação e encerramento de Web Workers. Drop de uma sessão interna de renderer não implementa o ciclo dessas APIs; testes específicos não verificados.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:285](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **11.6** Interceptação de requisições e funcionamento offline.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Interceptação de requisições e aplicações Web offline; Cache Storage e Service Workers ausentes. Abrir arquivo local autorizado não satisfaz este escopo.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:287](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md); [crates/browser-core/src/lib.rs](../../crates/browser-core/src/lib.rs); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **11.7** Worklets e APIs de execução especializadas quando necessárias.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Worklets e APIs especializadas não verificados; necessidade e escopo das aplicações-alvo precisam ser definidos antes de alegar cobertura.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:289](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 12. Imagens e gráficos Web

- [ ] **12.1** Formatos de imagem utilizados pelos sites, incluindo animação.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ PNG/JPEG, primeiro frame GIF/WebP, Data URLs e SVG estático limitado; dimensões naturais separadas da textura reduzida.
  - Origem: Decodificadores preexistentes + dimensões naturais separadas da textura de 08/11.
  - Lacunas/limites: Animação, AVIF e cobertura geral de formatos usados pelos sites.
  - Validação: Ciclo 16: browser-core 34/34, graphics 9/9, paint 7/7 e layout 26/26. Não valida APIs gráficas Web nem recuperação de contexto.
  - Fontes: [docs/roadmap/implementações Planejadas.md:295](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/image_decoder.rs](../../crates/browser-core/src/image_decoder.rs); [docs/specifications/images-subset.md](../../docs/specifications/images-subset.md); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **12.2** SVG integrado, estilos, texto, filtros, máscaras e referências.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ SVG rasterizado como imagem, com formas/caminhos/grupos/gradientes/clipping e estilos inline/referências internas de pintura limitados.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: SVG DOM integrado, texto/fontes, filtros, máscaras, use/hrefs e estilos completos, atualmente recusados.
  - Validação: Ciclo 16: browser-core 34/34, graphics 9/9, paint 7/7 e layout 26/26. Não valida APIs gráficas Web nem recuperação de contexto.
  - Fontes: [docs/roadmap/implementações Planejadas.md:297](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/image_decoder.rs](../../crates/browser-core/src/image_decoder.rs); [docs/specifications/images-subset.md](../../docs/specifications/images-subset.md); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **12.3** Imagens responsivas, seleção por resolução e carregamento progressivo.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: srcset/sizes/picture, seleção por resolução/densidade e carga progressiva. Dimensão intrínseca/proporção da imagem não implementa seleção responsiva.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:299](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md); [docs/specifications/images-subset.md](../../docs/specifications/images-subset.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **12.4** Canvas 2D, OffscreenCanvas e ImageBitmap.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Canvas 2D, OffscreenCanvas e ImageBitmap expostos à página. Rasterização interna em CPU/GPU não implementa essas APIs.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:301](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **12.5** WebGL e WebGPU para aplicações que dependem dessas APIs.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: APIs WebGL/WebGPU para conteúdo Web; wgpu com shader constante interno da shell não oferece essas APIs aos sites.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:303](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **12.6** Gestão de memória gráfica e recuperação após perda de contexto.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Limites internos de textura, vértices e atlas e caches GPU limitados.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Contextos Web e recuperação de context/device loss não implementados; não equivale a gestão completa de gráficos Web.
  - Validação: Ciclo 16: browser-core 34/34, graphics 9/9, paint 7/7 e layout 26/26. Não valida APIs gráficas Web nem recuperação de contexto.
  - Fontes: [docs/roadmap/implementações Planejadas.md:305](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 13. Áudio, vídeo e streaming

- [ ] **13.1** Elementos de mídia com estados, métodos e eventos corretos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ audio.duration somente leitura, real/NaN, com seed original único e invalidação controlada por src/preload.
  - Origem: Incremento WAV/audio.duration de 08/11.
  - Lacunas/limites: play/pause, estados/eventos/canplay e HTMLMediaElement completo; vídeo e reprodução não oferecidos.
  - Validação: Ciclo 16: browser-core 34/34 e bindings 130/130 incluem o subconjunto de metadados. Não houve prova de reprodução, vídeo ou codecs gerais.
  - Fontes: [docs/roadmap/implementações Planejadas.md:315](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/media.rs](../../crates/web-bindings/src/media.rs); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.2** Contêineres, demultiplexação e codecs.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Leitura de metadados RIFF/WAVE PCM little-endian tag 1, chunks fmt/data e coerência aritmética.
  - Origem: Parser de metadados WAV PCM do incremento de 08/11.
  - Lacunas/limites: Decodificação/reprodução de áudio e demultiplexação/codecs gerais de mídia; interpretar metadados WAV não é um pipeline de reprodução.
  - Validação: Ciclo 16: browser-core 34/34 e bindings 130/130 incluem o subconjunto de metadados. Não houve prova de reprodução, vídeo ou codecs gerais.
  - Fontes: [docs/roadmap/implementações Planejadas.md:317](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/media_metadata.rs](../../crates/browser-core/src/media_metadata.rs); [docs/specifications/audio-metadata-subset.md](../../docs/specifications/audio-metadata-subset.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.3** Sincronização de áudio/vídeo, buffering, busca temporal e reprodução adaptativa.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Sincronização, relógio, buffering, seek, reprodução adaptativa e streaming; duração calculada não satisfaz esses mecanismos.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:319](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md); [crates/browser-core/src/media_metadata.rs](../../crates/browser-core/src/media_metadata.rs); [crates/web-bindings/src/media.rs](../../crates/web-bindings/src/media.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.4** Media Source Extensions, Web Audio e WebCodecs.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Media Source Extensions, Web Audio e WebCodecs; nenhum backend correspondente confirmado nos bindings.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:321](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.5** Legendas, trilhas, fullscreen, picture-in-picture e Media Session.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Legendas, trilhas, fullscreen de mídia, picture-in-picture e Media Session não verificados. Maximizar janela nativa não é fullscreen de conteúdo Web.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:323](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/media.rs](../../crates/web-bindings/src/media.rs); [docs/specifications/audio-metadata-subset.md](../../docs/specifications/audio-metadata-subset.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.6** Captura de câmera, microfone e tela.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Captura de câmera, microfone e tela; pipeline atual recebe somente metadados locais, sem PCM, handles ou dispositivos.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:325](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md); [docs/adr/0018-brokered-local-audio-metadata.md](../../docs/adr/0018-brokered-local-audio-metadata.md); [crates/browser-core/src/media_metadata.rs](../../crates/browser-core/src/media_metadata.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.7** Decodificação por hardware quando disponível.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Decodificação de mídia por hardware; GPU de pintura não equivale a decoder de vídeo/áudio.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:327](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **13.8** Encrypted Media Extensions e integração com sistemas de descriptografia para serviços que os exigem.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: EME/CDM, DRM e fluxo de conteúdo cifrado não verificados; digest SHA não oferece descriptografia de mídia/DRM.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:329](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md); [docs/specifications/audio-metadata-subset.md](../../docs/specifications/audio-metadata-subset.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 14. Comunicação, identidade e dispositivos

- [ ] **14.1** WebRTC e comunicação de áudio, vídeo e dados em tempo real.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: WebRTC e áudio/vídeo/dados em tempo real expostos à página; HTTP interno e inferência não implementam essa API.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:339](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-bounded-crypto-digest.md](../../docs/releases/2026-11-08-bounded-crypto-digest.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **14.2** Web Crypto e geração segura de aleatoriedade.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ crypto.subtle.digest SHA-256/384/512, BufferSource copiado e Promise intrínseca, em HTTPS top-level confiável e sob quotas.
  - Origem: Incremento crypto.subtle.digest limitado de 08/11.
  - Lacunas/limites: RNG/getRandomValues/randomUUID, chaves, encriptação, assinaturas e algoritmo completo de secure contexts.
  - Validação: Ciclo 16: VM 440/440 e bindings 130/130 incluem digest/contextos. Não valida Web Crypto inteiro, RNG ou dispositivos.
  - Fontes: [docs/roadmap/implementações Planejadas.md:341](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-javascript/src/crypto.rs](../../crates/web-javascript/src/crypto.rs); [crates/web-bindings/src/crypto_context_tests.rs](../../crates/web-bindings/src/crypto_context_tests.rs); [docs/releases/2026-11-08-bounded-crypto-digest.md](../../docs/releases/2026-11-08-bounded-crypto-digest.md); [docs/adr/0019-bounded-crypto-https-context.md](../../docs/adr/0019-bounded-crypto-https-context.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **14.3** Autenticação, credenciais, WebAuthn e passkeys.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Autenticação Web, Credential Management, WebAuthn e passkeys; keyring de chaves do provedor de IA não implementa essas APIs da página.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:343](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-bounded-crypto-digest.md](../../docs/releases/2026-11-08-bounded-crypto-digest.md); [docs/releases/2026-11-08-http-content-type.md](../../docs/releases/2026-11-08-http-content-type.md); [crates/ai-runtime/src/lib.rs](../../crates/ai-runtime/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **14.4** Permissões, notificações e geolocalização.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Permissions, Notifications e Geolocation expostas à página não verificadas; permissões internas das ferramentas de IA são um mecanismo distinto.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:345](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **14.5** APIs de dispositivos utilizadas pelas páginas atendidas: gamepads, MIDI, USB, Bluetooth, serial e outras.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Gamepads, MIDI, USB, Bluetooth, serial e demais dispositivos Web; necessidade por site/hardware e cobertura ainda não demonstradas.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:347](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-bounded-crypto-digest.md](../../docs/releases/2026-11-08-bounded-crypto-digest.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **14.6** Integração com o sistema operacional e indicação correta de capacidades indisponíveis.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Plataforma/paralelismo reais, plugins/mimeTypes vazios e cookieEnabled/javaEnabled=false; sendBeacon retorna false sem enviar; shell Windows/keyring e recusa remota onde não há sandbox suportada.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Integração e feature detection Web IDL completos e suporte a dispositivos indisponíveis; recusas previsíveis não representam implementação das capacidades recusadas.
  - Validação: Ciclo 16: VM 440/440 e bindings 130/130 incluem digest/contextos. Não valida Web Crypto inteiro, RNG ou dispositivos.
  - Fontes: [docs/roadmap/implementações Planejadas.md:349](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/security-sandbox/src/lib.rs](../../crates/security-sandbox/src/lib.rs); [docs/releases/2026-11-08-bounded-crypto-digest.md](../../docs/releases/2026-11-08-bounded-crypto-digest.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 15. Segurança e isolamento

- [ ] **15.1** Separação entre interface, broker, renderers, GPU e decodificadores.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Crates direcionais, renderer em subprocesso e broker privilegiado fornecendo bytes; shell com GPU.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: GPU na aplicação e decodificadores no broker, sem todos os processos e isolamentos separados.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Fontes: [docs/roadmap/implementações Planejadas.md:359](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [crates/browser-core/src/image_decoder.rs](../../crates/browser-core/src/image_decoder.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/architecture/overview.md](../../docs/architecture/overview.md); [docs/security/threat-model.md](../../docs/security/threat-model.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **15.2** Sandbox efetiva e isolamento entre origens/contextos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ AppContainer Windows sem capabilities, allowlist de handles, Job de 512 MiB/processo único/kill-on-close e seis provas fail-closed; buckets por origem/sessão.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Sandbox Linux, isolamento de todas as classes/contextos e decodificadores/broker, e revisão independente.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Fontes: [docs/roadmap/implementações Planejadas.md:361](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/security-sandbox/src/lib.rs](../../crates/security-sandbox/src/lib.rs); [crates/security-sandbox/src/windows.rs](../../crates/security-sandbox/src/windows.rs); [crates/browser-process/src/session_storage.rs](../../crates/browser-process/src/session_storage.rs); [docs/adr/0004-windows-sandbox-and-http-broker.md](../../docs/adr/0004-windows-sandbox-and-http-broker.md); [docs/adr/0010-framed-sandbox-proof-and-host-launches.md](../../docs/adr/0010-framed-sandbox-proof-and-host-launches.md); [docs/adr/0017-bounded-session-storage.md](../../docs/adr/0017-bounded-session-storage.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **15.3** Validação de IPC e limites de memória, CPU, profundidade e trabalho.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Framing, versão/ID/payload/geometria, limites DOM/CSS/VM, deadlines e Job; Deserialize limitado e writer privado de 48 MiB antes do transporte.
  - Implementação concreta: ✅ Ciclo 16: codec separado de snapshot diagnóstico limitado a 12 MiB, com contagens/comprimentos/quotas agregadas verificados antes de alocar e rejeição de versões, truncamento e bytes excedentes; replay preserva origem e validação do supervisor/IPC.
  - Origem: Protocolo/validação/limites preexistentes + codec limitado de 08/11 e correção float_roundtrip posterior. Ciclo 16 acrescenta capture/replay explícito do CLI e codec de snapshots.
  - Lacunas/limites: Orçamento global de CPU/memória, memória transitória/strings/frame já alocados e fuzzing.
  - Lacunas/limites: Snapshot não é autenticado nem criptografado e seus bytes podem conter dados privados da página; não representa orçamento global, storage real ou replay completo de sessão/eventos.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança. CLI: 9 testes do codec, 5 integrações de capture/replay e 12 de subprocesso, total 26/26. Replay HTTPS usa AppContainer verificado e não busca recursos na rede; a prova negativa da sandbox é distinta de fetch da página.
  - Fontes: [docs/roadmap/implementações Planejadas.md:363](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-ipc/src/bounded.rs](../../crates/browser-ipc/src/bounded.rs); [crates/browser-ipc/src/encoding.rs](../../crates/browser-ipc/src/encoding.rs); [crates/browser-ipc/src/lib.rs](../../crates/browser-ipc/src/lib.rs); [crates/browser-process/src/lib.rs](../../crates/browser-process/src/lib.rs); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [docs/releases/2026-11-08-bounded-ipc-codec.md](../../docs/releases/2026-11-08-bounded-ipc-codec.md); [docs/adr/0020-bounded-ipc-codec.md](../../docs/adr/0020-bounded-ipc-codec.md); [apps/browser-cli/src/snapshot.rs](../../apps/browser-cli/src/snapshot.rs); [apps/browser-cli/src/main.rs](../../apps/browser-cli/src/main.rs); [apps/browser-cli/tests/snapshots.rs](../../apps/browser-cli/tests/snapshots.rs); [docs/specifications/diagnostic-snapshots.md](../../docs/specifications/diagnostic-snapshots.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **15.4** Same-origin policy, CSP e políticas de isolamento entre origens.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Regras limitadas de origem no broker/módulos/storage e bloqueio de conteúdo misto/DNS privado.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: SOP/CSP/CORS e isolamento Web entre origens completos; recursos públicos de CDN não são universalmente de mesma origem.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Fontes: [docs/roadmap/implementações Planejadas.md:365](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/src/module_assets.rs](../../crates/browser-core/src/module_assets.rs); [crates/browser-process/src/session_storage.rs](../../crates/browser-process/src/session_storage.rs); [docs/releases/2026-11-08-bounded-ipc-codec.md](../../docs/releases/2026-11-08-bounded-ipc-codec.md); [docs/adr/0008-authorized-subresources.md](../../docs/adr/0008-authorized-subresources.md); [docs/security/threat-model.md](../../docs/security/threat-model.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **15.5** Proteção de credenciais, armazenamento e permissões.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Chaves de IA no cofre do SO, perfil sem chaves/conteúdo, snapshots sem password/hidden, permissões por conversa/escopo e storage particionado limitado.
  - Origem: Cofre/perfil/permissões preexistentes + sessionStorage e observação limitada de 08/11.
  - Lacunas/limites: Credenciais Web e armazenamento amplo; texto/URLs comuns podem revelar segredos, sem sanitização geral.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Fontes: [docs/roadmap/implementações Planejadas.md:367](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/ai-runtime/src/lib.rs](../../crates/ai-runtime/src/lib.rs); [crates/browser-config/src/lib.rs](../../crates/browser-config/src/lib.rs); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [crates/ai-context/src/snapshot.rs](../../crates/ai-context/src/snapshot.rs); [docs/releases/2026-11-08-session-storage.md](../../docs/releases/2026-11-08-session-storage.md); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md); [docs/security/threat-model.md](../../docs/security/threat-model.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **15.6** Gestão de certificados e atualização de componentes vulneráveis.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Validação TLS padrão rustls e dependências fixadas em Cargo.lock.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Gestão avançada de certificados, pipeline de correções/auditoria de dependências e atualizador autenticado; hashes/Build ID não são atualização automática de vulnerabilidades.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Fontes: [docs/roadmap/implementações Planejadas.md:369](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/Cargo.toml](../../crates/browser-core/Cargo.toml); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [Cargo.lock](../../Cargo.lock); [docs/security/threat-model.md](../../docs/security/threat-model.md); [docs/distribution.md](../../docs/distribution.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **15.7** Tratamento seguro de crashes e conteúdo malformado.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Deadlines, encerramento/RAII/Job, framing/validação, rollback fatal e conservação do último checkpoint confirmado; testes de conteúdo malformado.
  - Origem: Supervisão/rollback preexistentes + limites IPC/VM e correções funcionais de 08–09/11.
  - Lacunas/limites: Robustez universal, fuzzing, ensaio prolongado e revisão independente das superfícies privilegiadas.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Fontes: [docs/roadmap/implementações Planejadas.md:371](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-process/src/lib.rs](../../crates/browser-process/src/lib.rs); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [crates/browser-core/src/media_metadata.rs](../../crates/browser-core/src/media_metadata.rs); [docs/releases/2026-11-08-bounded-ipc-codec.md](../../docs/releases/2026-11-08-bounded-ipc-codec.md); [docs/security/threat-model.md](../../docs/security/threat-model.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [x] **15.8** Acessos privilegiados mediados pelo broker, preservando o motor sem acesso direto a arquivos e rede.
  - **Estado: Implementado.**
  - Implementação concreta: ✅ No pipeline atual, browser-core resolve/canonicaliza/carrega recursos autorizados e entrega bytes/assets; renderer/web-* recebem dados tipados; resolutores SVG não fazem I/O. web-paint usa somente Write fornecido pelo host.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Nenhuma lacuna de implementação identificada nesse contrato. Não equivale a sandbox perfeita, eliminação de TOCTOU ou garantia universal de segurança.
  - Validação: Ciclo 16: IPC 23/23, process 18/18, security-sandbox 4/4 e os quatro testes de integração AppContainer executados, sem filtros. Não é auditoria universal de segurança.
  - Testes exatos aprovados no ciclo 16: appcontainer_proves_file_network_and_child_process_denial_before_remote_render; remote_live_document_retains_state_inside_the_verified_appcontainer; desktop_binary_verifies_appcontainer_and_renders_remote_bytes; desktop_local_and_sandboxed_sessions_can_start_concurrently.
  - Fontes: [docs/roadmap/implementações Planejadas.md:373](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-core/src/lib.rs](../../crates/browser-core/src/lib.rs); [crates/browser-core/src/network.rs](../../crates/browser-core/src/network.rs); [crates/browser-core/src/image_decoder.rs](../../crates/browser-core/src/image_decoder.rs); [crates/browser-process/src/lib.rs](../../crates/browser-process/src/lib.rs); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [docs/adr/0008-authorized-subresources.md](../../docs/adr/0008-authorized-subresources.md); [docs/adr/0014-brokered-static-modules.md](../../docs/adr/0014-brokered-static-modules.md); [docs/adr/0018-brokered-local-audio-metadata.md](../../docs/adr/0018-brokered-local-audio-metadata.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 16. Driver próprio de renderização do Noir

- [x] **16.1** \*\*Cena e display list próprias\*\*, produzidas pelo layout.
  - **Estado: Implementado.**
  - Implementação concreta: ✅ Layout próprio produz LayoutResult/DisplayItem; renderer forma RenderedPage; Scene::prepare gera triângulos/comandos/texturas/atlas próprios consumidos pelo backend da página.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Nenhuma lacuna de implementação no contrato de cena/display list próprias. Cobertura de CSS/efeitos e validação da apresentação pertencem aos demais subitens.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Testes exatos aprovados no ciclo 16: glyph_clip_tests::fully_visible_clip_preserves_geometry_order_and_images.
  - Fontes: [docs/roadmap/implementações Planejadas.md:379](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-layout/src/lib.rs](../../crates/web-layout/src/lib.rs); [crates/browser-ipc/src/lib.rs](../../crates/browser-ipc/src/lib.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.2** Árvores de propriedades para transforms, efeitos, clips e rolagem.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Pilha PushClip/PopClip, interseções aninhadas e transformações internas de viewport/zoom/rolagem da shell.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Árvores de propriedades de transforms, efeitos, clips e scroll completas.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:381](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.3** Rasterização de texto, imagens, vetores, bordas e efeitos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Glifos fontdue, quads/retângulos/bordas, texturas RGBA/alpha, SVG limitado rasterizado no broker e CPU próprio no CLI; glifos totalmente recortados não são rasterizados.
  - Origem: Driver CPU/GPU e decoder preexistentes + culling anterior à rasterização de 08/11.
  - Lacunas/limites: Vetores/efeitos/shaping e fidelidade geral completos.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:383](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [crates/browser-core/src/image_decoder.rs](../../crates/browser-core/src/image_decoder.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [x] **16.4** Atlas de glifos, texturas, buffers e gerenciamento de recursos.
  - **Estado: Implementado.**
  - Implementação concreta: ✅ Gerenciador do backend limitado com atlas R8 2048², 4096 combinações, budgets de raster/vértices/texturas, uploads e cache de duas cenas GPU com descarte; métricas separadas da materialização de glifos.
  - Origem: Atlas/texturas/buffers/cache preexistentes + métricas separadas da materialização de 08/11.
  - Lacunas/limites: Nenhuma parte nomeada do gerenciador atual está ausente. Limites: sem paginação do atlas, expansão geral ou recuperação device loss (esta última em 16.10); implementação não comprova execução GPU.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Testes exatos aprovados no ciclo 16: glyph_clip_tests::hidden_large_kinds_do_not_spend_atlas_but_visible_kinds_do.
  - Fontes: [docs/roadmap/implementações Planejadas.md:385](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.5** Composição de camadas, superfícies intermediárias e transparência.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Composição em ordem de draw, blending alpha e fronteiras dos controles/destaques.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Compositor de camadas e superfícies intermediárias completo.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:387](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.6** Invalidação parcial, tiles, descarte de áreas invisíveis e controle de sobreposição.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Cena retida/cache, culling de viewport, clipping e orçamento de overdraw; glifo fora do clip descartado antes do bitmap/atlas.
  - Origem: Cache/clipping/overdraw preexistentes + descarte de glifos invisíveis de 08/11.
  - Lacunas/limites: Invalidação parcial, tiles e occlusion completos; uploads de imagens não otimizados e texto fora do viewport preservado para rolagem.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:389](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/page_scene.rs](../../apps/browser-desktop/src/page_scene.rs); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.7** Agendamento de frames e sincronização com apresentação.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Pulsos rAF do host, timestamp real, até 32 callbacks por snapshot e identidade/tempo validados; oportunidade para aba exibida e elegível.
  - Origem: Loop nativo preexistente + pulsos rAF/IPC v8 de 08/11.
  - Lacunas/limites: Vsync, apresentação GPU e scheduler Web completo não garantidos.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:391](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [crates/web-bindings/src/animation.rs](../../crates/web-bindings/src/animation.rs); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [docs/releases/2026-11-08-animation-frames.md](../../docs/releases/2026-11-08-animation-frames.md); [docs/adr/0021-host-animation-frame-pulses.md](../../docs/adr/0021-host-animation-frame-pulses.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.8** Scroll e animações responsivos durante trabalho pesado da página.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Trabalho de documento em subprocesso/thread, cena retida durante scroll e UI nativa; pulsos de frame não enfileiram trabalho se ocupado.
  - Origem: Subprocessos/cena retida preexistentes + oferta descartável de frames de 08/11.
  - Lacunas/limites: Responsividade sob carga e frames independentes do compositor não demonstrados por benchmark; construção/raster/uploads também têm custo.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:393](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [apps/browser-desktop/src/page_scene.rs](../../apps/browser-desktop/src/page_scene.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/releases/2026-11-08-animation-frames.md](../../docs/releases/2026-11-08-animation-frames.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.9** Integração de canvas, conteúdo 3D e vídeo.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Integração de canvas, 3D e vídeo da página; imagens e shader interno não equivalem a esses caminhos.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:395](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [docs/releases/2026-11-08-natural-image-dimensions.md](../../docs/releases/2026-11-08-natural-image-dimensions.md); [docs/releases/2026-11-08-wav-metadata.md](../../docs/releases/2026-11-08-wav-metadata.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.10** Backend GPU, fallback CPU e recuperação após perda do dispositivo.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Backend próprio WGSL/wgpu e rasterizador CPU próprio no CLI.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Fallback CPU desktop completo e recuperação de perda de dispositivo.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:397](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [docs/releases/2026-11-08-clipped-glyph-raster.md](../../docs/releases/2026-11-08-clipped-glyph-raster.md); [docs/adr/0011-owned-web-graphics-driver.md](../../docs/adr/0011-owned-web-graphics-driver.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **16.11** Hit testing consistente com o conteúdo efetivamente apresentado.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Links/controles usam retângulos da display list com zoom/clipping; foco recortado e regiões de hit não crescem para acomodar labels.
  - Origem: Geometria de links/controles preexistente + contorno de foco recortado de 08/11.
  - Lacunas/limites: Hit testing DOM completo, transforms/stacking/overlap e identidade unificada de links fragmentados; validação visual ampla pendente.
  - Validação: Ciclo 16: graphics 9/9, paint 7/7, layout 26/26, renderer 15/15 e desktop unitário 35/35. Não equivale a validação GPU/visual/performance ampla.
  - Fontes: [docs/roadmap/implementações Planejadas.md:399](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-web-link-keyboard-focus.md](../../docs/releases/2026-11-08-web-link-keyboard-focus.md); [docs/validation/2026-11-03-controls-v051.md](../../docs/validation/2026-11-03-controls-v051.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 17. Automação e agente

- [ ] **17.1** API interna estável para observar DOM, layout, acessibilidade e imagem renderizada.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ BrowserSnapshot de texto/display list, links/controles confirmados com aba/geração; cópia limitada e JSON de até 10.000 bytes após filtro de permissões.
  - Origem: Observação interna preexistente + limite de extração/JSON de 08/11.
  - Lacunas/limites: API integral e estável de DOM/layout/acessibilidade/imagem e percepção visual.
  - Validação: Ciclo 16: ai-context 9/9 e desktop unitário 35/35. O teste de inferência real permanece ignorado; resistência ampla a prompt injection não comprovada. Repetição posterior: 24 chamadas (12 isoladas e 12 do grupo de quatro), totalizando 60 execuções de casos aprovadas, zero falhas; não são 60 testes novos.
  - Fontes: [docs/roadmap/implementações Planejadas.md:409](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [crates/ai-context/src/snapshot.rs](../../crates/ai-context/src/snapshot.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md); [target/validation/2026-11-09-snapshot-fixture-after.log](../../target/validation/2026-11-09-snapshot-fixture-after.log).

- [ ] **17.2** Navegação, seleção, digitação, rolagem e cliques com identificação de contexto.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Vocabulário Navigate/OpenTab/SwitchTab/Select/Type/Click validado por DocumentScope/IDs; seleção pode focar e rolar até elemento exposto.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Rolagem arbitrária, ações gerais DOM/coordenadas, elementos fora da projeção e outros contextos.
  - Validação: Ciclo 16: ai-context 9/9 e desktop unitário 35/35. O teste de inferência real permanece ignorado; resistência ampla a prompt injection não comprovada.
  - Fontes: [docs/roadmap/implementações Planejadas.md:411](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/adr/0006-ai-browser-tools.md](../../docs/adr/0006-ai-browser-tools.md); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **17.3** Esperas por carregamento, elementos, alterações e resultados.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Espera por commit real de navegação/evento com pending/queued, timeout de 90 s e invalidação da sequência por mudança de geração.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: wait-for-element, condições/mutações e esperas semânticas gerais ausentes do vocabulário.
  - Validação: Ciclo 16: ai-context 9/9 e desktop unitário 35/35. O teste de inferência real permanece ignorado; resistência ampla a prompt injection não comprovada.
  - Fontes: [docs/roadmap/implementações Planejadas.md:413](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [docs/adr/0006-ai-browser-tools.md](../../docs/adr/0006-ai-browser-tools.md); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **17.4** Cancelamento, rastreamento de ações e rejeição de comandos antigos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ interrupt_sequence/cancelled, estados pending/authorized/executing_action e resultado real rastreados; aba/geração conferidas antes de despachar/continuar, com limites de 8/32 ações.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Cancelamento não desfaz nem interrompe necessariamente operação já iniciada; rastreamento limitado ao fluxo da conversa, sem API geral de automação.
  - Validação: Ciclo 16: ai-context 9/9 e desktop unitário 35/35. O teste de inferência real permanece ignorado; resistência ampla a prompt injection não comprovada.
  - Fontes: [docs/roadmap/implementações Planejadas.md:415](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/ai_panel.rs](../../apps/browser-desktop/src/ai_panel.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md); [docs/adr/0006-ai-browser-tools.md](../../docs/adr/0006-ai-browser-tools.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **17.5** Permissões e proteção contra instruções maliciosas vindas das páginas.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Grants por escopo, permissões nativas, JSON fechado, contexto de página não confiável, revogação/filtro antes da cópia/envio e exclusão de password/hidden.
  - Origem: Permissões/escopo preexistentes + filtragem antes da cópia/JSON limitado de 08/11.
  - Lacunas/limites: Resistência ampla a prompt injection não demonstrada; texto/URLs podem conter segredos; modo total permite o vocabulário sem revisão por ação.
  - Validação: Ciclo 16: ai-context 9/9 e desktop unitário 35/35. O teste de inferência real permanece ignorado; resistência ampla a prompt injection não comprovada.
  - Fontes: [docs/roadmap/implementações Planejadas.md:417](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [crates/ai-runtime/src/lib.rs](../../crates/ai-runtime/src/lib.rs); [apps/browser-desktop/src/ai_panel.rs](../../apps/browser-desktop/src/ai_panel.rs); [docs/adr/0006-ai-browser-tools.md](../../docs/adr/0006-ai-browser-tools.md); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **17.6** WebDriver/WebDriver BiDi se houver objetivo de integração com ferramentas externas.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: WebDriver/BiDi não verificados. Item condicional: definir objetivo de integração externa; automação interna não implementa esses padrões.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:419](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-bounded-ai-observation.md](../../docs/releases/2026-11-08-bounded-ai-observation.md); [crates/ai-context/src/lib.rs](../../crates/ai-context/src/lib.rs); [docs/adr/0006-ai-browser-tools.md](../../docs/adr/0006-ai-browser-tools.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 18. Produto, acessibilidade e operação

- [ ] **18.1** Árvore de acessibilidade do conteúdo e integração com leitores de tela.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Árvore semântica do conteúdo e integração com leitores de tela; controles nativos da shell e contorno de foco não criam essa árvore.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:429](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/releases/2026-11-08-web-link-keyboard-focus.md](../../docs/releases/2026-11-08-web-link-keyboard-focus.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **18.2** Navegação por teclado, foco e preferências de acessibilidade.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Widgets egui/atalhos/foco, preferência reduce_motion e foco visível nativo dos links sob clipping.
  - Origem: Teclado/widgets/reduce_motion preexistentes + foco visível em links de 08/11.
  - Lacunas/limites: Ordem Tab DOM completa, tabindex, foco/eventos DOM, rolagem até todos os links e avaliação de acessibilidade.
  - Validação: Ciclo 16: browser-config 6/6 e desktop unitário 35/35; integrações AppContainer executadas. Não certifica acessibilidade, atualizador ou matriz ampla de SO/GPU.
  - Fontes: [docs/roadmap/implementações Planejadas.md:431](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [crates/browser-config/src/lib.rs](../../crates/browser-config/src/lib.rs); [docs/releases/2026-11-08-web-link-keyboard-focus.md](../../docs/releases/2026-11-08-web-link-keyboard-focus.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **18.3** Perfis, histórico, downloads, impressão e recuperação de sessões.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Perfil v2 com migração/backup, favoritos/workspaces/abas, restauração opt-in de endereços e histórico/scroll em memória.
  - Origem: Perfil/restauração preexistentes + offsets do histórico nativo de 08/11.
  - Lacunas/limites: Downloads/impressão não confirmados; recuperação completa DOM/VM/formulários e histórico/offsets após reinício. Raster/PPM do CLI não é impressão do navegador.
  - Validação: Ciclo 16: browser-config 6/6 e desktop unitário 35/35; integrações AppContainer executadas. Não certifica acessibilidade, atualizador ou matriz ampla de SO/GPU.
  - Fontes: [docs/roadmap/implementações Planejadas.md:433](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-config/src/lib.rs](../../crates/browser-config/src/lib.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/releases/2026-11-08-history-scroll.md](../../docs/releases/2026-11-08-history-scroll.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **18.4** Gestão de processos, abas suspensas e pressão de memória.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Até 16 RendererSession simultâneas com RAII/timeout/Job, sessões por aba, workspaces estacionados e carga inicial adiada.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Suspensão/descarte/recarga por pressão de memória e gerenciamento adaptativo completos não verificados.
  - Validação: Ciclo 16: browser-config 6/6 e desktop unitário 35/35; integrações AppContainer executadas. Não certifica acessibilidade, atualizador ou matriz ampla de SO/GPU.
  - Fontes: [docs/roadmap/implementações Planejadas.md:435](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/browser-process/src/session.rs](../../crates/browser-process/src/session.rs); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **18.5** Diagnósticos de console, rede, DOM, estilos e desempenho.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Inspector lista warnings de execução/CSS/layout e até 150 DisplayItem; HUD/CLI expõem tempos, nós, PID e memória. console.* grava mensagens limitadas em DocumentSession.state.logs, acessíveis por console_messages(), ainda sem canal no inspector.
  - Implementação concreta: ✅ Ciclo 16: erros preservam histórico/contextos internos limitados e rótulos de origem/fase/script/nó/tag/tipo; URLs saneadas e posições de parse somente com proveniência real, sem alterar message/thrown/Error.cause ou executar getters/coerções para diagnosticar.
  - Implementação concreta: ✅ CLI capture/replay explícito e limitado preserva DocumentInput e sua origem HTTPS, executa replay em AppContainer sem buscar recursos na rede e sem cookies/perfil/storage real. Arquivo preserva bytes de página, não é captura HTTP bruta nem replay de eventos.
  - Origem: Inspector/warnings/HUD e console interno preexistentes; ciclo 16 acrescenta diagnóstico contextual de exceções/bindings e capture/replay do CLI. Console continua sem ligação ao inspector.
  - Lacunas/limites: Console real ainda não ligado ao renderer/inspector; DevTools completos de rede/DOM/estilos/performance, profiling/edição/depuração, stack trace e proveniência de funções/callbacks persistidos pendentes. Nenhuma linha/coluna é inferida de offsets de JSON/eval/Function/link/runtime.
  - Validação: Ciclo 16: bindings 130/130, incluindo 17 testes de diagnóstico; VM 440/440; CLI 26/26 (9 codec, 5 capture/replay e 12 subprocesso). Fmt e Clippy globais aprovados. Essas verificações não conectam console ao inspector nem validam DevTools completos.
  - Testes exatos aprovados no ciclo 16: diagnostics::tests::runtime_json_and_dynamic_errors_do_not_claim_initial_source_positions; diagnostics::tests::unsupported_dom_src_identifies_receiver_and_type_without_value_or_coercion; diagnostics::tests::timer_diagnostic_identifies_src_receiver_without_fabricating_script_source.
  - Fontes: [docs/roadmap/implementações Planejadas.md:437](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [apps/browser-desktop/src/shell.rs](../../apps/browser-desktop/src/shell.rs); [apps/browser-desktop/src/hud.rs](../../apps/browser-desktop/src/hud.rs); [apps/browser-cli/src/main.rs](../../apps/browser-cli/src/main.rs); [crates/web-bindings/src/lib.rs](../../crates/web-bindings/src/lib.rs); [crates/web-renderer/src/lib.rs](../../crates/web-renderer/src/lib.rs); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [target/validation/google-current/diagnosis.md](../../target/validation/google-current/diagnosis.md); [crates/web-javascript/src/exception_diagnostics.rs](../../crates/web-javascript/src/exception_diagnostics.rs); [crates/web-bindings/src/diagnostics.rs](../../crates/web-bindings/src/diagnostics.rs); [crates/web-bindings/src/diagnostics_tests.rs](../../crates/web-bindings/src/diagnostics_tests.rs); [apps/browser-cli/src/snapshot.rs](../../apps/browser-cli/src/snapshot.rs); [apps/browser-cli/tests/snapshots.rs](../../apps/browser-cli/tests/snapshots.rs); [docs/specifications/script-diagnostics.md](../../docs/specifications/script-diagnostics.md); [docs/specifications/diagnostic-snapshots.md](../../docs/specifications/diagnostic-snapshots.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **18.6** Instalador, assinatura, atualização autenticada e rollback.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Scripts build/package/install-current, atalhos, Build ID/SHA-256, estágio e restauração da versão anterior se a promoção falha.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Assinatura e atualizador autenticado; rollback local de empacotamento não é atualização remota autenticada.
  - Validação: Ciclo 16: browser-config 6/6 e desktop unitário 35/35; integrações AppContainer executadas. Não certifica acessibilidade, atualizador ou matriz ampla de SO/GPU.
  - Fontes: [docs/roadmap/implementações Planejadas.md:439](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/distribution.md](../../docs/distribution.md); [scripts/build.ps1](../../scripts/build.ps1); [scripts/package.ps1](../../scripts/package.ps1); [scripts/install-current.ps1](../../scripts/install-current.ps1); [scripts/distribution-common.ps1](../../scripts/distribution-common.ps1); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **18.7** Compatibilidade com os sistemas operacionais e GPUs oficialmente atendidos.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Distribuição Windows x64, caminhos Windows/Linux, CI definida para ambos e backend wgpu com limites; evidências históricas Windows.
  - Origem: Base preexistente à rodada de recortes de 08/11; ver fontes do subsistema.
  - Lacunas/limites: Matriz oficial de SO/GPU/versões e ensaios atuais amplos; remoto Linux recusado sem sandbox. Workflow existente não comprova execução/resultado remoto.
  - Validação: Ciclo 16: browser-config 6/6 e desktop unitário 35/35; integrações AppContainer executadas. Não certifica acessibilidade, atualizador ou matriz ampla de SO/GPU.
  - Fontes: [docs/roadmap/implementações Planejadas.md:441](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [.github/workflows/ci.yml](../../.github/workflows/ci.yml); [apps/browser-desktop/src/page_gpu.rs](../../apps/browser-desktop/src/page_gpu.rs); [crates/security-sandbox/src/lib.rs](../../crates/security-sandbox/src/lib.rs); [docs/architecture/acceptance.md](../../docs/architecture/acceptance.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

### 19. Comprovação de compatibilidade

- [ ] **19.1** Conformidade JavaScript acompanhada pelo Test262.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Harness, execução e cobertura Test262 não comprovados; os 423 testes próprios da VM não são Test262 nem conformidade ECMAScript integral.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:447](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/architecture/acceptance.md](../../docs/architecture/acceptance.md); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md); [crates/web-javascript/src/lib.rs](../../crates/web-javascript/src/lib.rs).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **19.2** Conformidade da plataforma acompanhada pelos Web Platform Tests.
  - **Estado: Pendente.**
  - Implementação concreta: nenhuma capacidade específica confirmada para marcar como implementada.
  - Origem: Sem implementação específica confirmada para este escopo; capacidades relacionadas não são contadas como conclusão.
  - Lacunas/limites: Harness, execução e cobertura Web Platform Tests não comprovados; testes internos e fixtures não são WPT.
  - Validação: Não verificado como capacidade implementada. As suítes internas aprovadas não comprovam este recurso ausente/não demonstrado; consultar as lacunas e fontes.
  - Fontes: [docs/roadmap/implementações Planejadas.md:449](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/architecture/acceptance.md](../../docs/architecture/acceptance.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [.github/workflows/ci.yml](../../.github/workflows/ci.yml).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **19.3** Suítes específicas de gráficos e mídia.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Testes próprios de raster/atlas/clipping/overdraw, imagens e metadados WAV e fixtures específicas, com execuções unitárias registradas.
  - Origem: Testes/fixtures próprios preexistentes + regressões de 08/11 e execuções dos ciclos funcionais.
  - Lacunas/limites: Suítes completas GPU/visual e mídia/reprodução/codecs; aprovações CPU/geometria não validam apresentação GPU.
  - Validação: Ciclo 16: workspace 845 aprovados, 0 falhas, 1 inferência real ignorada, 0 filtrados. Resultado de testes próprios, sem Test262/WPT, soak ou auditoria independente.
  - Fontes: [docs/roadmap/implementações Planejadas.md:451](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [crates/web-graphics/src/lib.rs](../../crates/web-graphics/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [crates/browser-core/src/media_metadata.rs](../../crates/browser-core/src/media_metadata.rs); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **19.4** Regressões com páginas reais, incluindo busca, login, formulários e reprodução.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Histórico de Google/Wikipedia e regressões reduzidas de controles; smoke GUI de 09/11 no snapshot ciclo 13 mostrou JS/DOM ok na fixture compatibility-v06.html e logo/campo/botões na homepage Google.
  - Implementação concreta: ✅ Ciclo 16: nova captura pública da homepage pelo broker e replay fiel do DocumentInput HTTPS em AppContainer, com capture/replay saída 0, sem perfil/cookies/storage real e sem busca de recursos durante replay. O diagnóstico preservou exceções anteriormente mascaradas por _DumpException.
  - Implementação concreta: ✅ Regressão GUI separada do ciclo 16: digitou e submeteu uma consulta sintética pelo formulário nativo, navegou a Google Search e voltou à homepage pelo botão Voltar. O teste registrou a falha de renderização dos resultados, não busca funcional.
  - Origem: Registros históricos e smoke GUI ciclo 13 preservados; ciclo 16 acrescenta captura/replay explícitos e diagnóstico do mesmo conjunto congelado de bytes. São snapshots diferentes, não reclassificação retrospectiva do smoke antigo. Posteriormente, teste GUI separado de busca/Voltar em cópia identificada do ciclo 16; nenhuma correção Symbol/Math/performance foi incluída nesse snapshot.
  - Lacunas/limites: Incompatibilidades da homepage não corrigidas: script 2 registrou Host/native exotic descriptors are not implemented; scripts 8/9 registraram Cannot access a property of null or undefined antes de _DumpException. Timer recusou src em node=112, tag=script, value-type=string; CSS continua ignorado em parte. Não se inventa linha/coluna ou expressão causadora.
  - Lacunas/limites: Busca GUI foi testada e falhou em exibir resultados: consulta Noir browser test produziu apenas a mensagem de dificuldade da Pesquisa Google; warnings observados Value is not callable (random), performance is not defined e seletores CSS ignorados, sem comprovar causalidade completa. Voltar restaurou URL/posição do histórico, com campo vazio; não prova BFCache ou restauração de formulário. Não houve segunda busca, login, interação com CAPTCHA ou reprodução. Correções Symbol/Math/performance permanecem em andamento, fora da evidência validada.
  - Validação: Ciclo 16 tem dois ensaios distintos: (1) captura/replay CLI da homepage, SHA-256 672A7D6F29D9497484C13BD010558AC7A1F5C5A99611FC57F5D5CF4050F8F3D7, códigos 0, sem envio de consulta; (2) GUI de busca/Voltar, SHA-256 58D65BF453DB1A0A928C535E650DABFEF07C40139738F60A1483998E6420DCAB, iniciado aproximadamente em 2026-11-09T02:24:31Z com perfil temporário: submissão e Voltar funcionaram, lista de resultados não apareceu. Sem segunda busca/login/reprodução; nenhum CAPTCHA ou prompt de consentimento/permissão observado. Nenhum desses resultados valida correções posteriores.
  - Fontes: [docs/roadmap/implementações Planejadas.md:453](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/validation/2026-11-02-windows-v4.md](../../docs/validation/2026-11-02-windows-v4.md); [docs/validation/2026-11-03-controls-v051.md](../../docs/validation/2026-11-03-controls-v051.md); [docs/validation/2026-11-03-sandbox-v061.md](../../docs/validation/2026-11-03-sandbox-v061.md); [fixtures/form-control-sizing.html](../../fixtures/form-control-sizing.html); [docs/architecture/acceptance.md](../../docs/architecture/acceptance.md); [target/validation/google-current/result.json](../../target/validation/google-current/result.json); [target/validation/google-current/diagnosis.md](../../target/validation/google-current/diagnosis.md); [target/validation/google-current/google-home.png](../../target/validation/google-current/google-home.png); [target/validation/google-current/google-inspector.png](../../target/validation/google-current/google-inspector.png); [apps/browser-cli/src/snapshot.rs](../../apps/browser-cli/src/snapshot.rs); [apps/browser-cli/tests/snapshots.rs](../../apps/browser-cli/tests/snapshots.rs); [docs/specifications/script-diagnostics.md](../../docs/specifications/script-diagnostics.md); [docs/specifications/diagnostic-snapshots.md](../../docs/specifications/diagnostic-snapshots.md); [target/validation/google-diagnostics-cycle16/capture-session.json](../../target/validation/google-diagnostics-cycle16/capture-session.json); [target/validation/google-diagnostics-cycle16/google-home-replay.json](../../target/validation/google-diagnostics-cycle16/google-home-replay.json); [target/validation/google-search-cycle16/report.md](../../target/validation/google-search-cycle16/report.md); [target/validation/google-search-cycle16/result.json](../../target/validation/google-search-cycle16/result.json); [target/validation/google-search-cycle16/binary-and-session.json](../../target/validation/google-search-cycle16/binary-and-session.json); [target/validation/google-search-cycle16/search-result.jpg](../../target/validation/google-search-cycle16/search-result.jpg); [target/validation/google-search-cycle16/search-inspector.jpg](../../target/validation/google-search-cycle16/search-inspector.jpg); [target/validation/google-search-cycle16/back-home.jpg](../../target/validation/google-search-cycle16/back-home.jpg).
  - Evidência de execução/limites: [target/validation/google-current/result.json](../../target/validation/google-current/result.json); [target/validation/google-current/diagnosis.md](../../target/validation/google-current/diagnosis.md); [target/validation/google-current/snapshot.json](../../target/validation/google-current/snapshot.json); [target/validation/google-diagnostics-cycle16/capture-session.json](../../target/validation/google-diagnostics-cycle16/capture-session.json); [target/validation/google-diagnostics-cycle16/capture.log.exitcode](../../target/validation/google-diagnostics-cycle16/capture.log.exitcode); [target/validation/google-diagnostics-cycle16/replay.log.exitcode](../../target/validation/google-diagnostics-cycle16/replay.log.exitcode); [target/validation/google-diagnostics-cycle16/google-home-replay.json](../../target/validation/google-diagnostics-cycle16/google-home-replay.json); [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [target/validation/google-search-cycle16/report.md](../../target/validation/google-search-cycle16/report.md); [target/validation/google-search-cycle16/result.json](../../target/validation/google-search-cycle16/result.json); [target/validation/google-search-cycle16/binary-and-session.json](../../target/validation/google-search-cycle16/binary-and-session.json).

- [ ] **19.5** Fuzzing, testes prolongados, desempenho e revisão de segurança.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Modelo de ameaças, revisão estática, testes de malformados/limites/crash/rollback e métricas internas; histórico de falhas/correções preservado.
  - Origem: Modelo de ameaças/testes de limites preexistentes + revisão e ciclos de correção de 08–09/11.
  - Lacunas/limites: Fuzzing com corpus/sanitizers, soak, benchmarks controlados, auditoria independente e avaliações de prompt injection. Testes unitários não são fuzzing.
  - Validação: Ciclo 16: workspace 845 aprovados, 0 falhas, 1 inferência real ignorada, 0 filtrados. Resultado de testes próprios, sem Test262/WPT, soak ou auditoria independente.
  - Fontes: [docs/roadmap/implementações Planejadas.md:455](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/security/threat-model.md](../../docs/security/threat-model.md); [docs/architecture/acceptance.md](../../docs/architecture/acceptance.md); [crates/browser-process/src/lib.rs](../../crates/browser-process/src/lib.rs); [crates/web-paint/src/lib.rs](../../crates/web-paint/src/lib.rs); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

- [ ] **19.6** Matriz pública distinguindo implementado, parcial, indisponível e validado.
  - **Estado: Parcial.**
  - Implementação concreta: ✅ Matriz de capacidades, releases/subconjuntos com lacunas, relatórios de validação separados e este checklist rastreável de todos os 137 subitens.
  - Origem: Matriz/reports preexistentes + releases da rodada e inventário integral de 09/11.
  - Lacunas/limites: Publicação pública efetiva não verificada; manutenção contínua e cobertura de validação completa pendentes. Documentação no repositório não comprova publicação.
  - Validação: Ciclo 16: workspace 845 aprovados, 0 falhas, 1 inferência real ignorada, 0 filtrados. Resultado de testes próprios, sem Test262/WPT, soak ou auditoria independente.
  - Fontes: [docs/roadmap/implementações Planejadas.md:457](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md); [docs/architecture/capabilities.md](../../docs/architecture/capabilities.md); [docs/releases/2026-11-08-roadmap-first-pass.md](../../docs/releases/2026-11-08-roadmap-first-pass.md); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md); [docs/roadmap/checklist-implementacoes-2026-11-09.md](../../docs/roadmap/checklist-implementacoes-2026-11-09.md).
  - Evidência de execução/limites: [target/validation/2026-11-09-cycle16-tests.log](../../target/validation/2026-11-09-cycle16-tests.log); [docs/validation/2026-11-08-functional-baseline.md](../../docs/validation/2026-11-08-functional-baseline.md).

## Lacunas que continuam impedindo conclusão

- Plataforma Web: ECMAScript/Intl integrais, módulos assíncronos completos, Wasm com memória/imports/streaming, DOM/Web Components/observers e event loop completo.
- Conteúdo/visual: CSS/layout completos, shaping/bidi/fontes, canvas/WebGL/WebGPU, reprodução/streaming de mídia e compositor/recuperação GPU completos.
- Aplicações reais: Fetch/XHR/streams, cookies/localStorage/IndexedDB/workers/offline, History/contextos aninhados, POST/upload, identidade/dispositivos e acessibilidade do conteúdo.
- Produto/prova: console ainda não conectado ao inspector no snapshot auditado, correção das incompatibilidades Google agora identificadas, assinatura/atualização autenticada, Test262/WPT, estabilidade prolongada e auditoria independente.

## Integridade e método

A extração cobre somente os bullets de **Escopo de referência original** de [docs/roadmap/implementações Planejadas.md](../../docs/roadmap/implementa%C3%A7%C3%B5es%20Planejadas.md). A numeração e os textos originais não foram substituídos pelos 19 títulos nem pelos recortes implementados. Contagens: 8, 4, 9, 6, 7, 8, 9, 6, 8, 7, 7, 6, 8, 6, 8, 11, 6, 7, 6.

SHA-256 do roadmap original na extração: B225FA8CFCB727ED667A735C2E22C53DE28F2ED62FCFDF5669FB661FBFF75DB2. O inventário foi consolidado por leitura de fontes, contratos, releases e logs existentes. Não executou Cargo, GUI ou rede; não editou código nem o roadmap original. Os resultados funcionais citados foram executados separadamente e conferidos nos logs.

Executável entregue preservado: dist/Noir/noir-validacao-2026-11-09.exe, 83.667.968 bytes, SHA-256 8B6C0C9E004742C8B3F986B2988F806F44409E3C834838C6FE2E3949A6C1DFDE. Ele não é automaticamente atualizado pelas fontes ou pelo snapshot de teste/smoke.

Referências de rodada: [docs/releases/2026-11-08-roadmap-first-pass.md](../../docs/releases/2026-11-08-roadmap-first-pass.md); [docs/releases/2026-11-08-array-from-dom.md](../../docs/releases/2026-11-08-array-from-dom.md); [docs/releases/2026-11-08-webassembly-i64-bigint.md](../../docs/releases/2026-11-08-webassembly-i64-bigint.md); [docs/releases/2026-11-08-shared-dom-selectors.md](../../docs/releases/2026-11-08-shared-dom-selectors.md); [docs/releases/2026-11-08-animation-frames.md](../../docs/releases/2026-11-08-animation-frames.md).
