# ADR 0007 — Efeitos de script anteriores a uma exceção

Data: 2026-11-02. Estado: implementado; avaliação funcional pendente.

## Problema concreto

O HTML atual recebido de uma busca do Google inclui um callback que remove um estilo de ocultação de um elemento e em seguida chama navigator.sendBeacon. A chamada de telemetria não é suportada. A política anterior de rollback integral do callback restaurava o estilo oculto e desfazia também a alteração útil. Outros sites que fazem uma alteração de UI antes de uma integração ausente sofrem do mesmo problema.

A conclusão não é que Google Search passou a ser compatível. O documento observado é uma página de redirecionamento/ativação de JavaScript; outros scripts ainda exigem eval, classes privadas e APIs ausentes.

## Decisão

Erros ordinários Runtime e throw preservam os efeitos já realizados. A execução interrompida retorna um diagnóstico; listeners restantes e tarefas prontas continuam dentro dos limites. A sintaxe do script completo ainda é analisada antes de executar qualquer instrução. try/catch/finally trata exceções ordinárias, preservando o valor de throw e respeitando conclusões de retorno, break e continue.

Erros Resource não são exceções controladas pela página. Não podem ser capturados por catch ou substituídos por retorno de finally. A operação que esgotou recursos restaura DOM, VM e estado anteriores, conservando orçamento consumido; scripts são desativados e timers/listeners removidos. Não há nova autoridade de arquivos, rede ou broker.

Essa separação melhora a semântica compatível sem simular sendBeacon, não cria um cliente HTTP no motor e não suprime diagnósticos de APIs ausentes. Navegações do shell permanecem transacionais e respostas antigas continuam proibidas de substituir o documento atual.

## Limites e verificação

Não há implementação completa de Error prototypes, modos estritos, relatórios de erro do navegador, event loop ou event propagation. A coleta de diagnósticos informa o primeiro erro ordinário de um lote; os demais callbacks continuam sujeitos ao mesmo orçamento da sessão.

A implementação passou por compilação e Clippy incluindo os alvos existentes de testes. O teste existente de rollback de exceção ordinária foi atualizado para conferir preservação de efeitos e recusa de uma mutação DOM inválida. Os testes não foram executados, conforme solicitação do usuário; não há alegação de validação funcional ou segurança concluída para este incremento.

Referências: [ECMA-262 — Try Statement](https://tc39.es/ecma262/multipage/ecmascript-language-statements-and-declarations.html#sec-try-statement), [ECMA-262 — New Operator](https://tc39.es/ecma262/multipage/ecmascript-language-expressions.html#sec-new-operator).
