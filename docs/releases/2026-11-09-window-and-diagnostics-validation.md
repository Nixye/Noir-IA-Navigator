# Validação integrada: Window e diagnóstico de recursos

Este ciclo reúne [descritores do global Window](2026-11-09-window-global-descriptors.md), [diagnóstico de cargas dinâmicas](2026-11-09-script-load-diagnostics.md) e diagnósticos numéricos limitados do parser/arrays. O caso sintético de definição de Window.Map por um polyfill passou. As quotas de fontes, tokens, parser, arrays e strings não foram aumentadas.

## Evidência executada

Validação integrada local Linux, Rust 1.99, concluída em 2026-11-09 às 04:20 UTC:

- `cargo fmt --all --check`: aprovado.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: aprovado.
- `cargo test --workspace --locked`: 984 testes passaram, zero falhas e um ignorado, em 43 grupos de resultados.
- O teste ignorado, `bundled_model_produces_real_output`, exige modelo/runtime verificados para inferência real.
- `git diff --check`: sem erros.
- Os hashes dos 969 arquivos acompanhados permaneceram idênticos durante os checks. Código, testes, fixtures e configuração continuaram idênticos até a publicação. Somente documentação foi ajustada após a validação: esta nota foi acrescentada, notas públicas foram revisadas e material privado foi excluído da entrega.
- A fixture `javascript-window-descriptors.html` foi renderizada em subprocesso pelo CLI, com zero warnings e texto confirmado “Polyfill global e descritores ativos.”

A revisão para publicação não encontrou credenciais, chaves privadas ou dados pessoais no incremento. Strings de privacidade nos testes são marcadores sintéticos. Nenhuma captura privada, perfil de navegador, log privado ou harness temporário integra a entrega.

## Limites e próxima verificação

Estes resultados não comprovam Google Search funcional. A nova revisão ainda exige QA Windows e navegação no AppContainer para verificar o comportamento real em sites.

A validação local não inclui GUI, execução Windows, inferência real ou suítes Test262/WPT completas. Os executáveis distribuídos anteriormente não foram substituídos. A CI Windows/Linux deve ser conferida no commit publicado; o sucesso local não implica aprovação da CI.
