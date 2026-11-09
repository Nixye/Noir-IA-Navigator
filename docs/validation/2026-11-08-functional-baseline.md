# Baseline funcional e ciclos de correção — 2026-11-08

## Estado atual

Validação em andamento, atualizada em 2026-11-09 UTC. O ciclo 16 confirmou o workspace com **845 aprovados, 0 falhas, 1 ignorado e 0 filtrados**, saída 0, incluindo os casos AppContainer e a nova cobertura de diagnóstico/captura. Os gates finais de fmt e Clippy também tiveram saída 0. Web-javascript lib passou 440/440. Esses resultados pertencem aos snapshots executados; novas alterações posteriores precisam de validação própria e não herdam esse resultado.

O [smoke Google de 2026-11-09](2026-11-09-google-smoke.md), inicialmente realizado em uma cópia do ciclo 13, mostrou a homepage parcialmente renderizada, com erros JavaScript e CSS. A captura/replay do ciclo 16 revelou erros anteriores a `_DumpException`: descriptors de objetos host/native no script 2 e acesso a propriedade de null/undefined nos scripts 8/9. O timer identifica agora atribuição `src` ao nó 112, tag `script`, valor string. Isso melhora o diagnóstico, não resolve o Google nem comprova a operação exata que originou cada erro. Busca, login e reprodução de mídia não foram testados. A instrução posterior da usuária autoriza testes e ciclos de correção até um sistema funcional. Isso substitui o adiamento do item 19 para o trabalho novo, mas não transforma compilações ou fontes de testes anteriores em resultados funcionais.

Itens 1–18 permanecem abertos/parciais. Item 19: validação em andamento. Há evidência delimitada de apresentação gráfica e navegação à homepage, não de navegador pronto, compatibilidade geral ou estabilidade prolongada. Inferência real continua ignorada na suíte ampla.

## Histórico preservado

A primeira passagem dos itens 1–18 e os recortes Array.from, WebAssembly i64/BigInt e seletores DOM/CSS do segundo ciclo tiveram revisão/checagem estática. Não tiveram testes/fixtures executados naquele momento. As notas de release conservam essa evidência histórica, inclusive o adiamento então vigente.

## Ciclo 1: baseline JavaScript

- Comando: `cargo test -p web-javascript --lib --offline --locked --no-fail-fast`.
- Diretório: raiz do repositório Noir em DARKFRAME.
- Registro: `target/validation/2026-11-08-javascript-baseline.log`.
- Estado: concluído com falha, código de saída Cargo 101 confirmado pelo executor. Compilação do alvo de teste concluída em 45,12 s; o harness anunciou 412 testes, mas abortou antes do resumo final. Isso não significa 412 testes executados.
- O wrapper PowerShell usa Tee-Object para conservar o log; ele não faz parte do comando Cargo acima.
- Falha fatal: stack overflow em `coercion::tests::conversion_recursion_is_a_resource_error_not_a_catchable_throw`. Processo de teste terminou com `0xc00000fd, STATUS_STACK_OVERFLOW`. Não há totais finais válidos de aprovados/reprovados.

## Frames e próximos registros (estado histórico no ciclo 1)

O recorte requestAnimationFrame/IPC v8 tem fontes liberadas e revisão consolidada sem bloqueadores; os testes de frames ainda estão pendentes. Fontes de regressão não são resultados. Cada ciclo deve registrar comando, saída final, falhas concretas, correção e repetição relevante sem apagar o resultado anterior.

Smoke de aplicativo, fixtures, rede, GPU e IA: nenhum resultado novo confirmado neste relatório. Um smoke futuro deve identificar cenário e limites; abrir uma janela ou pintar uma fixture não demonstra compatibilidade geral. Não houve publicação, commit, push ou reconstrução de dist/Noir nesta etapa documental.

### Falhas observadas antes do aborto

O log completo contém 12 linhas FAILED abaixo. São ocorrências observadas, não resumo de toda a suíte; o aborto impediu a impressão dos diagnósticos finais de cada falha.

- `array_from::tests::mapper_validation_and_bad_iterators_never_fall_back`
- `array_methods::bigint_conversion_tests::bigint_values_remain_legal_and_errors_do_not_mutate_arrays`
- `array_from::tests::supported_receivers_and_explicit_constructor_rejection`
- `async_iteration::tests::absent_sync_return_does_not_assimilate_argument`
- `async_iteration::tests::absent_sync_throw_closes_without_reading_result_or_awaiting_it`
- `async_iteration::tests::sync_operations_preserve_arguments_and_read_done_before_value`
- `async_iteration::tests::direct_operations_preserve_argument_count_and_cached_next`
- `coercion::tests::arrays_use_string_hints_for_nested_join_conversions`
- `bigint::tests::wrappers_have_real_prototypes_and_brand_checks`
- `coercion::tests::string_conversion_preserves_utf16_and_symbol_exception_scope`
- `coercion::tests::key_errors_do_not_replay_or_continue_effects`
- `crypto::tests::conversions_reject_without_spoofed_buffer_getters`

Próximo ciclo: corrigir/isolar o overflow e investigar as falhas concretas antes de novas features. Repetições terão registros separados; este resultado de falha permanece preservado.

## Ciclo 2: revisão e checagem estática de frames

Revisão consolidada concluída sem bloqueadores. O executor central confirmou `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked`, sessão 9828, saída 0 e duração de 2,52 s. Trata-se de formatação/checagem estática: testes rAF, fixture, desktop e apresentação GPU continuam sem resultado confirmado.

## Ciclo 3: coerção recursiva e correções nativas da VM

O overflow do ciclo 1 recebeu um guard separado de call_depth: no máximo oito conversões objeto→primitivo simultâneas, incluindo getters/hooks e chaves. Primitivos não ocupam esse guard. Reentrada finita do mesmo objeto é permitida; a nona conversão ativa falha como Resource não capturável. O guard é liberado nos retornos de sucesso/erro, e rollback não reembolsa instruções. Não houve aumento de stack nem retirada do teste. Esse limite não é prova de segurança de toda a pilha ou de outros caminhos arbitrariamente profundos da VM.

Resultados conferidos nos logs em target/validation:

- `2026-11-08-conversion-recursion.log`: teste original do overflow, 1 aprovado, 0 reprovados; compilação 29,73 s. Não houve stack overflow nessa execução focada.
- `2026-11-08-conversion-suite.log`: 22 testes, 19 aprovados e 3 reprovados, incluindo quatro regressões novas aprovadas. Permanecem arrays_use_string_hints_for_nested_join_conversions, key_errors_do_not_replay_or_continue_effects e string_conversion_preserves_utf16_and_symbol_exception_scope, já observados no baseline.
- `2026-11-08-native-array_from-after.log`: 11 aprovados, 0 reprovados.
- `2026-11-08-native-array_methods-after.log`: 1 aprovado, 1 reprovado.
- `2026-11-08-native-bigint-after.log`: 12 aprovados, 1 reprovado.
- `2026-11-08-native-crypto-after.log`: 6 aprovados, 2 reprovados.
- `2026-11-08-native-reflection-after.log`: 3 aprovados, 1 reprovado.

Os últimos grupos não constituem uma suíte global aprovada. Os grupos native-*-after foram executados diretamente com `.\target\debug\deps\web_javascript-14b3c06d20988b94.exe <filtro> --nocapture`, usando os filtros `array_from::`, `array_methods::`, `bigint::`, `crypto::` e `reflection::`; Tee-Object conservou cada log. A regressão `reflection::tests::errors_have_real_prototype_chains_without_name_branding` passou no grupo reflection. As contagens acima foram lidas dos respectivos resumos finais, não inferidas de fontes.

A correção de Error/TypeError e demais construtores suportados cria protótipos/cadeias reais para instanceof, sem decidir pela propriedade name. A falha reproduzida recusava o construtor com `instanceof requires a supported function constructor`. Isso não completa Error: own name/message legados, toString, cause, stack e conversão de mensagem continuam limitados. O registro de 33 handlers Array preexistentes foi concluído em fontes e segue em validação separada; não se declara aprovado aqui. Nova execução `cargo test -p web-javascript --lib --offline --locked --no-fail-fast`, sessão 3078, log `target/validation/2026-11-08-javascript-after-native-fixes.log`, terminou com novo aborto, detalhado no ciclo 4 abaixo.
## Ciclo 4: nova suíte JavaScript após correções nativas

Comando: `cargo test -p web-javascript --lib --offline --locked --no-fail-fast`. Sessão 3078, log `target/validation/2026-11-08-javascript-after-native-fixes.log`. Compilação 30,27 s. Cargo terminou com saída 101; processo com `0xc00000fd, STATUS_STACK_OVERFLOW`, agora em `tests::untrusted_execution_and_allocation_are_bounded`.

O harness anunciou 418 testes. O log contém 355 linhas ok e 38 FAILED antes do aborto, sem resumo final: não são totais de uma suíte concluída nem 418 execuções confirmadas. A correção focada de coerção do ciclo 3 permanece válida no seu escopo, mas não eliminou todos os caminhos de overflow. A nova falha permanece em investigação.

O registro dos handlers Array preexistentes está integrado em fontes. As regressões novas de Error/Array e os casos antes falhos de wrappers BigInt, array_methods e join de arrays passaram nesta execução parcial, conforme executor; isso não certifica toda a VM. Nenhuma asserção foi removida para produzir aprovação.

Checagem focada posterior: `.\target\debug\deps\web_javascript-14b3c06d20988b94.exe properties:: --nocapture`, log `target/validation/2026-11-08-native-properties-after.log`: 10 aprovados, 2 reprovados. Falhas: primitive_string_delete_uses_virtual_utf16_properties_without_boxing e for_in_scopes_prototypes_control_flow_and_deletions_are_bounded. A preparação com surrogate escapes fora do lexer suportado está em triagem; mudar essa preparação não demonstra aprovação antes de repetir os testes.
### Comandos exatos do ciclo 3 confirmados posteriormente

- Overflow de coerção: `cargo test -p web-javascript --lib --offline --locked coercion::tests::conversion_recursion_is_a_resource_error_not_a_catchable_throw -- --exact --nocapture`.
- Grupo coerção: `target/debug/deps/web_javascript-14b3c06d20988b94.exe 'coercion::tests::' --nocapture --test-threads=1`.

## Ciclo 5: preparação de fontes de teste dentro do subconjunto existente

Foram alteradas apenas fontes de teste em crypto.rs, coercion.rs, reflection.rs e properties.rs. Getters em literais, ausentes da gramática suportada, foram substituídos por Object.defineProperty com flags equivalentes. Surrogate escapes recusados pelo lexer foram substituídos por String.fromCharCode, preservando as mesmas unidades UTF-16 e asserções. Não houve ampliação de lexer/parser nem mudança de produção neste recorte. Os resultados abaixo não validam as sintaxes substituídas.

Compilação: `cargo test -p web-javascript --locked --no-run`, sessão 22865, saída 0; log `target/validation/2026-11-08-test-source-build.log`, 6,69 s. No snapshot do executável com 418 testes, os grupos focados retornaram:

- `2026-11-08-test-source-crypto.log`: 8 aprovados, 0 reprovados.
- `2026-11-08-test-source-reflection.log`: 4 aprovados, 0 reprovados.
- `2026-11-08-test-source-properties.log`: 12 aprovados, 0 reprovados.
- `2026-11-08-test-source-coercion.log`: 21 aprovados, 1 reprovado, string_conversion_preserves_utf16_and_symbol_exception_scope.

O autor confirmou formatação edition2024 dos quatro arquivos, com registro `target/validation/2026-11-08-test-source-fmt.log`. Os testes focados acima não são uma execução dos 418 casos nem uma aprovação da suíte completa.

### Alteração posterior ainda sem resultado

Depois desses resultados, o último surrogate literal na asserção BigInt do teste string_conversion, linha 631 naquele snapshot, também foi adaptado. Essa edição posterior ainda não foi recompilada/retestada neste registro. Não converter o resultado 21/22 em 22/22 com base apenas na edição.

O segundo overflow de execução direta continua sob investigação. Um teto de profundidade de chamadas de 16 é somente candidato neste momento, sem validação registrada. A triagem de async/resumable prossegue separadamente. A prioridade permanece corrigir e medir o baseline, sem novas features ou alegação de sistema funcional.
## Ciclo 6: teto de chamadas e primeiro baseline completo da VM

O default de call_depth foi reduzido de 32 para 16. Runtime::new também aplica teto absoluto 16, inclusive quando o chamador pede usize::MAX; limites menores são preservados. O guard de oito coerções simultâneas permanece, mas o teto de chamadas pode ser atingido antes em String recursivo; ambos produzem Resource. Não houve aumento de stack. Esta é uma restrição estreita do subconjunto, não prova universal de segurança da pilha nativa.

Resultados, em target/validation:

- `2026-11-08-call-depth-original.log`: `cargo test -p web-javascript --lib --offline --locked tests::untrusted_execution_and_allocation_are_bounded -- --exact --nocapture`; compilação 29,09 s; 1 aprovado, 0 reprovados.
- `2026-11-08-call-depth-regressions.log`: `target/debug/deps/web_javascript-14b3c06d20988b94.exe 'tests::call_depth_' --nocapture --test-threads=1`; 2 aprovados, 0 reprovados. Casos finitos diretos/mútuos/getters e limite fatal/cleanup/rollback exercitados.
- `2026-11-08-conversion-after-call-ceiling.log`: grupo coerção, 22 aprovados, 0 reprovados. Inclui a preparação de fonte posterior ao resultado 21/22 do ciclo 5.
- `2026-11-08-javascript-after-call-ceiling.log`: executável da VM com `--nocapture`; suíte completa terminou sem aborto, **385 aprovados e 35 reprovados**, 0 ignorados/filtrados, 0,22 s; saída 101. Agora há resumo final válido para 420 casos, mas a suíte continua reprovada.

As 35 falhas permanecem em triagem. Correções async/resumable e de preparação de fontes posteriores ainda precisam de nova compilação/execução. O desaparecimento dos dois abortos nesses cenários não permite afirmar que todo caminho de recursão seja seguro ou que o navegador seja funcional.

## Ciclo 7: baseline independente de crates

Comando: `cargo test -p web-wasm -p web-dom -p web-html -p web-css -p web-layout -p web-paint -p web-graphics -p browser-ipc -p ai-context --lib --offline --locked --no-fail-fast`.

Log: `target/validation/2026-11-08-independent-crates-baseline.log`. Sessão 19222, saída 101, compilação 9,21 s. Resumos finais: **130 aprovados e 1 reprovado**.

- ai-context: 9 aprovados.
- browser-ipc: 22 aprovados, 1 reprovado.
- web-css: 35 aprovados.
- web-dom: 3 aprovados.
- web-graphics: 9 aprovados.
- web-html: 9 aprovados.
- web-layout: 26 aprovados.
- web-paint: 7 aprovados.
- web-wasm: 10 aprovados.

Nenhum caso ignorado/filtrado. A falha é `browser-ipc::tests::animation_frame_timestamp_bounds_and_request_round_trip`, na igualdade do timestamp após JSON. Precisão numérica é hipótese em investigação, não causa concluída neste registro. Este resultado já é evidência funcional parcial de IPC, inclusive uma falha concreta de frames; não valida callbacks rAF de ponta a ponta.

Os testes não inicializaram GPU nem executaram rede, AppContainer ou aplicativo. Aprovações de web-graphics não são validação de apresentação gráfica. Smoke de navegador, estabilidade prolongada e compatibilidade geral continuam pendentes.

### Fontes posteriores, ainda sem execução confirmada

Há uma correção de produção em lib.rs para que construct com alvo não-Function devolva property_type_error (caso new Math()). Separadamente, fontes de teste JSON passaram a verificar surrogates isolados como unidades UTF-16 positivas; fixtures de getters/chaves computadas foram reexpressas com defineProperty e flags equivalentes; tabelas antigas de sintaxe/RegExp foram reconciliadas com capacidades já existentes, preservando os negativos reais e a rejeição antes de efeitos. O autor informou que essas alterações aguardam compilação/testes. Não se classificam as 35 falhas como resolvidas nem se atribui expansão de gramática a adaptações de teste.

## Ciclo 8: parser for-init, fixtures resumíveis e nativas

`cargo test -p web-javascript --lib --locked --offline --no-run` compilou o snapshot com 423 testes em 29,80 s. Log `target/validation/2026-11-08-resumable-focused.log`; execuções diretas de web_javascript-14b3c06d20988b94.exe com --nocapture retornaram:

- `resumable::tests::`: 153 aprovados e 3 reprovados.
- `async_iteration::tests::`: 4 aprovados.
- `parser::`: 21 aprovados.
- `prepared_call::tests::`: 15 aprovados.

A correção de produção do parser é restrita à sondagem de for-init: expression(10) confundia yield sem operando com alvo for-in. O fallback agora exige bare_yield e erro Syntax; Resource continua propagado. O caso original com yield no início/teste/update do for e nova regressão de limites passaram. Isso não libera toda posição suspensível.

Outras mudanças desse grupo foram preparação e expectativas de testes: chaves computadas/getters via APIs equivalentes; negativos recusados no parser verificam Syntax; capacidades já existentes passaram a ter asserções positivas; labeled-for-await permanece explicitamente recusado. Limite de 33 argumentos continua verificado no parser e por AST manual no runtime, com apenas os primeiros 32 efeitos e nenhum cleanup após Resource. Não são novas features nem redução de asserções para esconder falhas.

`target/validation/2026-11-08-native-fixtures-focused.log` registra 11 alvos exatos aprovados, cada execução --exact com saída 0 e 422 filtrados: namespace Math, JSON escapes, host Reflect, import dinâmico, capacidade typed array, duas tabelas gerais de sintaxe/RegExp e quatro casos Wasm. Esse resultado confirma os casos alterados, não toda a suíte.

## Ciclo 9: handshake de fixtures e única falha restante

Log `target/validation/2026-11-08-resumable-handshake-diagnostic.log`: nova compilação --no-run em 3,63 s. Fixtures async generator passaram a consumir o Await final de return, mantendo os outputs exatos. Os casos exatos de update e assignment passaram; delete ainda falhou.

Execução completa `web_javascript-14b3c06d20988b94.exe --nocapture`, log `target/validation/2026-11-08-javascript-after-handshake.log`: **422 aprovados, 1 reprovado**, 0 ignorados/filtrados, 0,21 s, sem aborto. A única falha foi member_delete_suspends_without_getvalue_or_replaying_in_all_kinds. O diagnóstico identificou objeto efêmero entregue ao consumidor Rust sem raiz conservada durante GC; não demonstrou defeito de produção no GC. O patch de rooting da fixture e a nova rodada estão em andamento, ainda sem resultado final neste ciclo. Não declarar 423/423 por inferência.

## Ciclo 10: precisão JSON dos timestamps IPC

O log `target/validation/ipc-float-before.log` comprovou a causa da falha do ciclo 7: JSON com timestamp 9007199254740991.0 era decodificado como 9007199254740990.0. O teste exato falhou (0 aprovados, 1 reprovado). A configuração workspace de serde_json passou a habilitar float_roundtrip, sem nova dependência nem alteração de Cargo.lock, segundo executor.

Resultados posteriores:

- `target/validation/ipc-float-after.log`: browser-ipc 23 aprovados, 0 reprovados; compilação 2,73 s, execução 0,90 s. O bloco adicional de doc-tests tem 0 casos e não aumenta essa contagem.
- `target/validation/ipc-float-temporal.log`: testes de frames browser-process 2 aprovados (16 filtrados) e web-renderer 3 aprovados (12 filtrados); compilação 10,18 s.

Esses resultados substituem a hipótese de precisão por causa reproduzida e correção testada, preservando a falha histórica. São cobertura funcional parcial de tempo/IPC/renderização, sem execução de desktop, GPU, rede ou prova nova de AppContainer. A validação global e o smoke continuam pendentes.
## Ciclo 11: suíte JavaScript aprovada no snapshot final

A fixture de delete passou a conservar value.clone() em raiz externa durante o consumo Rust, com verificação de owner/objeto vivo e outputs preservados. A produção de GC não foi alterada para essa correção.

- `cargo test -p web-javascript --lib --offline --locked resumable::tests::member_delete_suspends_without_getvalue_or_replaying_in_all_kinds -- --exact --nocapture`: 1 aprovado, 0 reprovados, 422 filtrados, saída 0; compilação 3,58 s. Log `target/validation/2026-11-08-resumable-delete-root-final.log`.
- `cargo test -p web-javascript --lib --offline --locked`: **423 aprovados, 0 reprovados, 0 ignorados/filtrados**, saída 0; preparação 0,17 s e testes 0,21 s. Log `target/validation/2026-11-08-javascript-final-green.log`.

Este ciclo encerra a triagem da suíte lib de web-javascript desse snapshot, preservando as falhas e abortos anteriores. Não equivale a conformidade ECMAScript, suíte completa do workspace, teste de browser/desktop, GPU, rede, AppContainer ou aplicativo funcional. A validação do navegador continua em andamento.
## Ciclo 12: baseline amplo de formatação, Clippy e workspace

Registros em target/validation, com códigos de saída conferidos nos arquivos .log.exitcode:

- `baseline-fmt-20261108T221134Z.log.exitcode`: saída 0. O arquivo textual .log não foi produzido/encontrado; o código confirmado registra a conclusão sem diagnósticos textuais disponíveis.
- `baseline-clippy-20261108T221141Z.log` e .log.exitcode: saída 101. Foram reportados 54 diagnósticos em web-javascript e 1 em browser-ipc; o resumo do alvo lib JavaScript menciona 37 e o lib test 54, que não devem ser somados como conjuntos independentes.
- `baseline-tests-20261108T221158Z.log`, .log.exitcode e .log.summary.json: saída 101. Soma dos resumos finais: **774 aprovados, 11 reprovados, 1 ignorado e 4 filtrados**. Esses quatro filtros não foram executados nem aprovados.

Distribuição das falhas: desktop 34 aprovados/1 reprovado; web-bindings 100/8; web-renderer 13/2. Web-javascript continuou 423/423. O teste ignorado é de inferência real. Os quatro testes filtrados são de AppContainer (dois em subprocess e dois em worker_transport); oito testes locais de subprocesso foram executados e passaram. Isso não substitui a execução dos quatro casos filtrados, inferência real ou smoke visual de desktop.

Leitura inicial das fontes sugere preparação de fixtures antigas com sintaxe não suportada e expectativas sobre segmentação do texto entre as causas possíveis. A investigação e correção estão em andamento; nenhum desses diagnósticos é tratado aqui como causa final nem as 11 falhas como resolvidas. Os arquivos produtivos ainda estão sendo editados e precisam de nova rodada confirmada.

Comandos confirmados pelo executor, via cargo na raiz .: Este ciclo não satisfaz o gate de Clippy ou testes e não demonstra navegador funcional, nova validação AppContainer, GPU ou compatibilidade geral.
Comandos do ciclo 12:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo test --workspace --offline --locked --no-fail-fast -- --skip appcontainer_proves_file_network_and_child_process_denial_before_remote_render --skip remote_live_document_retains_state_inside_the_verified_appcontainer --skip desktop_binary_verifies_appcontainer_and_renders_remote_bytes --skip desktop_local_and_sandboxed_sessions_can_start_concurrently
```

Não houve --ignored. As exclusões acima são parte do escopo real e não devem ser omitidas ao reproduzir ou descrever o baseline.

## Ciclo 13: correções de bindings, seletores e fixtures

Registros de 2026-11-09 em `target/validation`, com comandos e códigos de saída no próprio log:

- `2026-11-09-cycle13-fmt.log`: `cargo fmt --all --check`, saída 0.
- `2026-11-09-cycle13-clippy.log`: `cargo clippy --workspace --all-targets --offline --locked -- -D warnings`, saída 101. Restaram dois `collapsible_if` em `web-layout`; a repetição de diagnósticos entre lib e lib test não representa quatro defeitos independentes.
- `2026-11-09-cycle13-tests.log`: mesmo comando de testes do ciclo 12, inclusive os quatro `--skip` AppContainer. Soma dos resumos finais: **792 aprovados, 1 reprovado, 1 ignorado e 4 filtrados**, saída 101. `web-bindings` passou 113/113, `web-css` 36/36, `web-renderer` 15/15 e `web-javascript` 423/423.

A única falha foi `shell::tests::snapshot_bounds_unicode_home_and_active_and_parked_tabs_without_mutation`, no alvo desktop, com ID observado 2 em vez de 100. Sua investigação e repetição estão no ciclo 14; não se considera essa execução aprovada retrospectivamente.

Correções delimitadas, conferidas nas fontes e nos casos aprovados:

- A sondagem da propriedade ausente `then` em document/nós DOM válidos devolve `Undefined`, permitindo assimilação de Promise sem transformar a ausência em erro. Validação do handle, cobrança de leitura, limites e erros das demais APIs não suportadas permanecem. Regressões em `crates/web-bindings/src/promise_host_tests.rs` cobrem identidade, microtasks, GC e falhas fatais.
- Fixtures de getters foram reexpressas com `Object.defineProperty`; expectativas de unidades UTF-16/surrogates e do vetor SHA-256 foram corrigidas. Essas adaptações de teste não representam expansão da gramática JavaScript nem nova implementação criptográfica.
- Identificadores CSS crus como `#123` são rejeitados, mantendo formas escapadas como `#\31 23`. A regressão em `crates/web-css/src/selector_query.rs` verifica tanto os negativos como o encontro de nós reais pelos escapes.
- Testes do renderer verificam o texto completo no DOM e os fragmentos de palavras na display list, incluindo largura e distância de um espaço entre fragmentos. Não exigem que uma frase inteira ocupe um único `DisplayItem::Text`, nem removem a verificação do conteúdo.
- A fixture headless de egui libera `textures_delta` sem painter. Isso é limpeza da fixture, não evidência de execução GPU.
- Dois novos testes de rAF em subprocesso real passaram: `retained_animation_frames_preserve_batch_order_and_do_not_advance_timers` e `retained_animation_frames_reject_bad_timestamps_without_losing_the_session`, em `apps/browser-cli/tests/subprocess.rs`. São sessões locais controladas; não demonstram animação de sites arbitrários.

## Ciclo 14: reprodução da fixture e conclusão dos checks focados

### Snapshot desktop antes e depois

`target/validation/2026-11-09-snapshot-fixture-reproduction.log` identifica o executável de teste anterior à correção, SHA-256 `3555780D1FAD376D9024509254A8349C12D2997C46EE5DC97397FE0B638031B1`, e registra 24 chamadas: 12 isoladas e 12 com o grupo de quatro testes de snapshot. Houve quatro falhas nas isoladas e três nas chamadas em grupo, sempre na expectativa 2 versus 100. As demais chamadas passaram; a falha era intermitente.

A preparação inseria outro grupo em `parked`, tornando a expectativa ordenada dependente da iteração do `HashMap`. A fixture agora confirma que existe um único grupo estacionado, identifica sua chave, verifica que não é o workspace ativo e substitui esse grupo existente. Mantém as asserções de limites Unicode, ordem, contexto e ausência de mutação. Essa correção não altera o algoritmo produtivo de ordenação de snapshots nem prova que toda ordem de `HashMap` seja estável.

`target/validation/2026-11-09-snapshot-fixture-after.log` identifica o executável posterior, SHA-256 `AE558F1F9387D2146F8F4B82ABB49C56686DD71E70EFF28D0FC2DA13C8903521`: **24 chamadas com saída 0, totalizando 60 execuções de casos e nenhuma falha** (12 × 1 isolado + 12 × 4 em grupo). São repetições de quatro testes, não 60 testes distintos nem uma nova suíte global. Os logs registram modos/iterações; não conservam uma linha completa de comando para essas invocações diretas.

### Lint e AppContainer

- `2026-11-09-cycle14-clippy.log`: saída 101, com diagnósticos `collapsible_match`, `collapsible_if` e `manual_is_multiple_of` em `browser-core`.
- `2026-11-09-lint-worker-pass1-clippy.log`: saída 101, com quatro diagnósticos em `browser-desktop` (divisibilidade, chunks e dois `if` colapsáveis). O log de fmt da primeira passagem registra saída 0.
- `2026-11-09-lint-worker-pass2-fmt.log` e `2026-11-09-lint-worker-pass2-clippy.log`: ambos registram `CARGO_EXITCODE=0`. O Clippy concluiu o perfil dev em 1,47 s. Esses dois logs não contêm o comando completo; o escopo fmt/Clippy corresponde à passagem informada pelo executor, e não a uma nova execução feita para redigir este documento.
- `2026-11-09-appcontainer-cli.log`: **2 aprovados, 0 falhas, 10 filtrados**, saída 0, 4,38 s. Casos: `appcontainer_proves_file_network_and_child_process_denial_before_remote_render` e `remote_live_document_retains_state_inside_the_verified_appcontainer`.
- `2026-11-09-appcontainer-desktop.log`: **2 aprovados, 0 falhas, 0 filtrados**, saída 0, 14,61 s. Casos: `desktop_binary_verifies_appcontainer_and_renders_remote_bytes` e `desktop_local_and_sandboxed_sessions_can_start_concurrently`.

Os quatro casos AppContainer deixam de ser apenas filtros pendentes neste ciclo. Seus resultados cobrem os cenários de isolamento/transporte definidos nos testes, não uma auditoria completa de segurança nem compatibilidade de navegação. O ciclo 15 os executou novamente dentro da suíte ampla, sem exclusões.

## Ciclo 15: workspace aprovado, sem filtros AppContainer

Comando na raiz do repositório em DARKFRAME:

```powershell
cargo test --workspace --offline --locked --no-fail-fast
```

`target/validation/2026-11-09-cycle15-workspace-tests.log` registra preparação do perfil de teste em 15,85 s, todos os resumos e `CARGO_EXITCODE=0`. A soma conferida é **797 aprovados, 0 reprovados, 1 ignorado e 0 filtrados**. Os doc-tests com zero casos não aumentam a contagem. Não houve `--skip` nem `--ignored`.

Recortes relevantes: subprocessos CLI 12/12 (incluindo rAF e os dois AppContainer), desktop binário 35/35, transporte desktop 2/2, `web-bindings` 113/113, `web-css` 36/36, `web-javascript` 423/423 e `web-renderer` 15/15. Os quatro casos AppContainer aprovados aqui já estão incluídos nos 797; não devem ser somados novamente a partir dos logs focados.

O caso ignorado é `ai-runtime::tests::bundled_model_produces_real_output`, que requer modelo/runtime verificados e execução com `--ignored`. Portanto este ciclo não valida inferência real. Também não demonstra reprodução multimídia, login, buscas, aplicações complexas, estabilidade prolongada ou conformidade geral Web/ECMAScript.

### Identidade dos artefatos e limite de validade

- O artefato entregue `dist/Noir/noir-validacao-2026-11-09.exe` permanece com SHA-256 `8B6C0C9E004742C8B3F986B2988F806F44409E3C834838C6FE2E3949A6C1DFDE`, conferido por leitura de hash. `2026-11-09-executable-delivery.json` registra 83.667.968 bytes, build e consulta de versão com saída 0, versão `Noir 0.9.0 (development)`. Ele não foi reconstruído nesta atualização documental e não deve receber os resultados de outro binário por associação de nome/data.
- O smoke Google usou `target/validation/google-current/noir-cycle13.exe`, SHA-256 `66C535CCDE8AA1B0621EC6075E332E0B785A0E079D0C344C0A997C4F651A3CC5`, diferente do entregue. Seu resultado parcial e limitações estão no [relatório específico](2026-11-09-google-smoke.md).
- As edições posteriores para melhorar diagnóstico/captura não estão cobertas pelos checks acima. Exigem nova compilação, fmt, Clippy, testes relevantes e repetição do cenário antes de qualquer nova conclusão funcional.

Esta atualização apenas leu fontes/evidências e editou documentação de validação. Não executou Cargo, rede ou GUI e não alterou código, roadmap/checklist ou dist. Os resultados verdes confirmam as suítes dos snapshots registrados; o objetivo de um navegador funcional continua aberto.

## Ciclo 16: diagnóstico de exceções, captura/replay e nova suíte ampla

Este ciclo valida as mudanças de diagnóstico e captura que ainda não estavam cobertas no fechamento do ciclo 15. Não reclassifica as falhas históricas como passes nem certifica correções posteriores.

### JavaScript: cobertura nova, aborto reproduzido e correção estrutural

- `target/validation/2026-11-09-exception-diagnostics-focused.log`: **17 aprovados, 0 falhas, 423 filtrados**. Os casos novos cobrem histórico de exceções capturadas, limites/truncamento, contexto de host, async/generators/microtasks, ausência de getters/coerção no diagnóstico e preservação do objeto lançado, nome, mensagem e `cause` da página.
- `2026-11-09-exception-diagnostics-crate.log`: a primeira execução ampla anunciou 440 testes, mas abortou por `STATUS_STACK_OVERFLOW` (`0xc00000fd`) no caso existente `parser::resumable_syntax_tests::classical_for_yield_probe_keeps_resource_errors_fatal`. Não há resumo global válido dessa tentativa: anunciar 440 não significa ter executado ou aprovado 440.
- `2026-11-09-exception-diagnostics-stack-repro.log`: a execução exata desse teste reproduziu o mesmo aborto. Assim, os 17 casos focados verdes não eram suficientes para encerrar a mudança.
- A correção separou `atom`, `atom_other` e `template_literal` em `crates/web-javascript/src/parser.rs`, reduzindo os temporários carregados pelo caminho recursivo de agrupamentos. Conservou o limite 64, a entrada adversarial de 70 pares de parênteses e a expectativa de erro `Resource`. Segundo o executor, não houve `RUST_MIN_STACK` nem override de stack; não se aumentou a pilha para mascarar a falha.
- `2026-11-09-exception-diagnostics-stack-fixed.log`: **1 aprovado, 0 falhas, 439 filtrados**. `2026-11-09-exception-diagnostics-crate-after.log`: **440 aprovados, 0 falhas, 0 ignorados/filtrados**, 0,28 s; doc-tests adicionais sem casos. Isso confirma a regressão e a suíte desse snapshot, não todas as formas possíveis de nesting ou ausência geral de stack overflow.

### Bindings: falhas de fixtures novas preservadas

`target/validation/2026-11-09-diagnostics-bindings-01.log` e `.log.exitcode` registram **128 aprovados, 2 falhas**, saída 101:

- `caught_dom_messages_preserve_long_properties_and_controls_but_warnings_are_bounded`: a propriedade da fixture excedia 1024 bytes e atingia o limite de recurso antes do diagnóstico pretendido (`Property name exceeds 1024 bytes`).
- `unsupported_dom_src_identifies_receiver_and_type_without_value_or_coercion`: getter literal não suportado causava erro de parser `Expected ","`, antes de alcançar `Unsupported DOM assignment src`.

As fixtures foram corrigidas sem ampliar quotas ou gramática: propriedade Unicode de 500 bytes, ainda acima do limite de apresentação de 120 e dentro de 1024, e getter equivalente via `Object.defineProperty`. Permanecem verificações da mensagem observável, controles/Unicode, limite do aviso, tipo/tag/id do receptor, ausência de coerção/getter e de exposição do valor atribuído.

`target/validation/2026-11-09-diagnostics-bindings-02.log`: **130 aprovados, 0 falhas, 0 ignorados/filtrados**, 0,03 s. O aumento sobre o ciclo 15 é de 17 casos de diagnóstico.

### Captura e replay pelo CLI

`target/validation/2026-11-09-snapshot-cli-01.log`: **26 aprovados, 0 falhas, 0 ignorados/filtrados**, somando 9 testes do codec, 5 de integração de snapshots e os 12 subprocessos já existentes. Não são 26 testes novos: o aumento é de 14.

A cobertura verifica limites antes de leitura/alocação, truncamentos, bytes excedentes, identidade de formato/build, URLs com credenciais, origem preservada, storage indisponível/vazio, roundtrip de bytes/assets/viewport, escrita sem sobrescrever arquivos e replay sem buscar novamente os arquivos originais. O caso `remote_snapshot_replay_preserves_https_origin_inside_the_normal_appcontainer` passou. Captura é explícita pelo broker e não executa scripts; replay remoto conserva HTTP/HTTPS e segue o caminho normal de subprocesso AppContainer, sem transformar a página em arquivo local ou executá-la em VM sem sandbox.

### Gates finais do ciclo 16

Comandos exatos confirmados pelo executor, nesta ordem, via `cargo` na raiz `.`:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo test --workspace --offline --locked --no-fail-fast
```

Os logs não embutem essas linhas completas de comando; os códigos de saída estão nos arquivos separados:

- `2026-11-09-cycle16-clippy.log` e `.log.exitcode`: primeira tentativa com saída 101, por um único `field_reassign_with_default` na fixture de diagnóstico JavaScript. A correção usa `Runtime { call_depth: 1, ..Runtime::default() }`, sem adicionar `allow`.
- `2026-11-09-cycle16-fmt-after.log.exitcode`: saída 0. Não há arquivo textual `cycle16-fmt-after.log`; nenhum diagnóstico textual foi conservado.
- `2026-11-09-cycle16-clippy-after.log` e `.log.exitcode`: saída 0; término do perfil dev em 3,70 s.
- `2026-11-09-cycle16-tests.log` e `.log.exitcode`: saída 0; preparação do perfil de teste em 58,98 s. **41 resumos, incluindo grupos com zero casos, somam 845 aprovados, 0 falhas, 1 ignorado e 0 filtrados.** Não houve `--skip` ou `--ignored`.

Os 48 casos adicionados desde o ciclo 15 se distribuem em 17 JavaScript + 17 bindings + 9 codec + 5 integração CLI. Os resultados focados acima já estão incluídos nos 845 e não devem ser somados novamente. O caso ignorado continua sendo `ai-runtime::tests::bundled_model_produces_real_output`; inferência real permanece sem validação neste ciclo.

### Google capturado e reproduzido: diagnóstico melhor, incompatibilidade aberta

Em `target/validation/google-diagnostics-cycle16/`, `capture-session.json`, `capture.log.exitcode` e `replay.log.exitcode` registram a captura pública de `https://www.google.com/` e replay, ambos com saída 0. O snapshot preserva URL HTTPS, viewport 1024 × 768 e storage indisponível com zero entradas. Não houve perfil real, cookies, login, busca ou reprodução de mídia. Captura e replay não significam ausência de erros na página; o JSON de replay conserva os avisos.

- CLI: `browser-cli-cycle16.exe`, SHA-256 `672A7D6F29D9497484C13BD010558AC7A1F5C5A99611FC57F5D5CF4050F8F3D7`.
- Snapshot: `google-home.noir-snapshot`, SHA-256 `0E0CA5F51A811D8A6C0C649EFBF70139764CAF18E39DE888C580991D0FC216F6`.
- Saídas: `google-home-replay.json` e `google-home-replay.ppm`. Os avisos agora mostram `Host/native exotic descriptors are not implemented` antes do erro secundário do script 2, `Cannot access a property of null or undefined` antes dos erros dos scripts 8/9 e `DOM receiver node=112 kind=element tag=script value-type=string` no timer `src`. Os avisos CSS persistem.

O possível envolvimento de instalação de `Symbol.dispose` via `defineProperty` é **hipótese de inspeção estática**, não a causa exata comprovada pelo replay, nem correção executada neste ciclo. A evidência de execução identifica os erros anteriores, mas ainda não uma cadeia causal completa ou uma solução. Detalhes, comandos e limites estão no [relatório Google](2026-11-09-google-smoke.md).

O hash do executável entregue foi novamente conferido e permanece `8B6C0C9E004742C8B3F986B2988F806F44409E3C834838C6FE2E3949A6C1DFDE`. Ele não é o CLI de captura/replay e não recebe por associação os resultados deste ciclo. Esta atualização documental não executou Cargo, rede ou GUI nem modificou código, checklist ou dist. Suíte verde e diagnóstico reproduzível ainda não equivalem a Google funcional ou navegador pronto.
