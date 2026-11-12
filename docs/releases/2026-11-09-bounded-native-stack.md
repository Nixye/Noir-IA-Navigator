# Pilha nativa limitada durante JavaScript

## Escopo

O parser e os dispatchers recursivos de execução separaram variantes em helpers pequenos. Assim, avaliar uma soma ou entrar em um bloco não reserva as variáveis temporárias de todas as outras operações. O despacho de chamadas nativas também fica fora do quadro das chamadas ordinárias da VM. A coleta de declarações `var` e a validação estática de strict mode e escopos usam travessias iterativas, preservando as verificações e a ordem das falhas.

A pilha configurada dos processos e threads de produção não foi ampliada. Os limites de chamadas (16), avaliação de expressões (64), tokens (32.768) e nós sintáticos (16.384) permanecem. A profundidade estrutural efetiva das expressões também é conferida durante o parse: combinar grupos e cadeias não pode multiplicar o limite de 64 antes da validação ou destruição da AST.

A execução conta até 64 statements simultaneamente ativos, inclusive entre chamadas e reentrâncias. Ao exceder esse limite, retorna `Resource: Statement evaluation depth exceeded`. Esse erro é fatal para a execução do script: `catch` e `finally` não podem suprimi-lo. Os contadores são liberados na saída por erro e o rollback continua preservando o trabalho consumido.

## Verificação

As regressões usam threads de 1 MiB para representar uma pilha pequena sem alterar a configuração de produção. Cobrem cadeias de expressões, blocos, chamadas finitas, acessores herdados, erros fatais, rollback e recuperação. A fixture `fixtures/javascript-stack-budgets.html` combina execução limitada e atualização real do DOM.

A validação sintética não demonstra compatibilidade completa de sites. A execução em cada plataforma e o transporte entre processos devem ser verificados separadamente.
