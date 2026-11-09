# Validação integrada: inline dinâmico e coleções vivas

O ciclo reúne scripts inline dinâmicos síncronos, currentScript e checkpoints reentrantes limitados ([ADR 0024](../adr/0024-synchronous-dynamic-inline-scripts.md)), além de getElementsByTagName em Document/Element com coleções vivas ([nota do incremento](2026-11-09-live-tag-collections.md)). Não acrescenta I/O ao motor nem altera a autorização de navegação remota.

## Evidência executada

Validação local integrada em Linux, Rust 1.99, concluída em 2026-11-09:

- Formatação do workspace: aprovada.
- Clippy do workspace e de todos os targets, com warnings tratados como erros: aprovado.
- Suíte completa: 941 testes passaram, zero falhas e um ignorado, em 43 grupos de resultados. O teste ignorado é `bundled_model_produces_real_output`, que exige modelo/runtime verificados para inferência real.
- Os hashes dos 958 arquivos de fonte, configuração, documentação e fixtures acompanhados permaneceram idênticos durante a execução dos checks. A auditoria anterior à publicação confirmou novamente esse mesmo estado; apenas esta nota de evidência foi acrescentada depois.
- `git diff --check`: sem erros.
- Fixture de coleções renderizada no CLI: zero warnings e texto confirmado “Coleção viva e padrão de loader ativos.” Testes de subprocesso também verificaram inline síncrono, continuidade na sessão, erro ordinário, rollback fatal e replay sem novas buscas externas.

A revisão do incremento para publicação não encontrou credenciais, chaves privadas, dados pessoais ou capturas privadas. As fontes de testes são sintéticas; as URLs novas pertencem a domínios de exemplo.

## Limites da evidência

A validação acima é local Linux. Não inclui inspeção gráfica da interface, execução local Windows/AppContainer, replay do Google, navegação remota real, Test262/WPT completos ou inferência real. A compatibilidade funcional de Google Search, YouTube ou aplicações arbitrárias não está declarada. Os executáveis já distribuídos não foram substituídos.

O resultado da CI de Windows/Linux deve ser consultado para o commit publicado; o sucesso local não implica aprovação da CI.
