# Listas de chamadas reservadas e grupos iterativos

## Listas mais largas sob o orçamento existente

Funções JavaScript admitem até 64 parâmetros e chamadas/construções até 64 argumentos. A largura é conferida no parser e nos caminhos de execução, incluindo apply, Reflect, bind, classes/super e chamadas suspensas. Não muda o contrato separado de 32 parâmetros do WebAssembly. O limite inclui todos os argumentos de uma chamada: `f.call` usa uma posição para o receiver, e `Function` usa uma posição para o corpo. Não há exceções ocultas para adapters.

Buffers variáveis de Value passam a reservar capacidade antes de alocar ou copiar, compartilhando as 65.536 células cumulativas já usadas por arrays. Listas de aridade conhecida reservam uma vez; listas com spread crescem geometricamente até 64 e cobram a capacidade adicional e o trabalho de realocação. Mover um vetor já reservado não cobra uma cópia inexistente. Cópias para `arguments`, rest, apply/Reflect e combinação de bound arguments são distintas e recebem suas próprias reservas.

Esse teto não foi ampliado. Chamadas repetidas podem esgotá-lo antes do que ocorria quando seus buffers temporários não eram contabilizados. GC, rollback, descarte e exceções não devolvem essas reservas. A cobrança é de capacidade solicitada e trabalho, não uma medição exata de RSS ou de todo armazenamento interno da VM.

Spread de chamada acrescenta diretamente à lista final. Não materializa até 4.096 valores para só depois descobrir o limite da chamada. Um iterador genérico pode exigir o 65º `next` para distinguir término de excesso; Resource continua fatal e não dispara cleanup com novos efeitos. Spread suspensível não foi habilitado. Isso elimina o vetor intermediário do builder de argumentos, mas não transforma todos os fallbacks de iterator em streaming: alguns caminhos existentes ainda materializam unidades de string ou valores de coleções, sob seus limites próprios.

## Parâmetros e bindings

A quantidade de parâmetros não é a quantidade de nomes produzidos por seus padrões. Um visitante emprestado mede nomes antes de instalar o ambiente, sem criar repetidamente vetores de strings. Permanecem os limites de padrões, 1.024 bindings por scope e 16.384 bindings vivos totais. Bytes de chaves copiados para bindings passam pelo orçamento cumulativo de strings antes da cópia; identificadores não recebem artificialmente o limite de tamanho das propriedades.

Defaults, rest, `this`, `arguments`, `.length`, TDZ e efeitos dos getters conservam a ordem do subconjunto. Os limites de heap, scopes, instruções, strings e fontes dinâmicas continuam independentes. Function segue analisando parâmetros e corpo separadamente, e mantém o teto de fonte de 64 KiB. Uma função larga ainda pode ser recusada por outra quota, sem apoio parcial silencioso.

## Agrupamentos sem frames nativos redundantes

Parênteses não criam nós da AST. O parser agora consome grupos iniciais que não sejam listas de arrow e retoma seus tails de expressão iterativamente. O conteúdo usa precedência mínima zero; apenas o último tail volta à precedência original. Isso preserva casos como `((a)+b)*c`, chamadas agrupadas, targets e receivers. Arrows são reconhecidas em cada abertura e continuam sujeitas às regras de quebra de linha.

Cada grupo ainda consome a visita do parser que consumia antes. Não há reset do contador nativo dentro de funções nem ampliação da pilha de produção. Chamadas, padrões, statements e outros caminhos ainda recursivos conservam o guard compartilhado de 64. Cada tail que muda a árvore é validado antes de outra composição; grupos inalterados não percorrem repetidamente a mesma AST larga. A profundidade estrutural de expressões permanece 64, inclusive para árvores parciais rejeitadas.

## Verificação

Regressões de grupos executam parsing, Clone/Drop e casos reais de execução em threads de 1 MiB. Cobrem associação, precedência, arrows, ASI, yield, templates, alvos e receivers, 40 chamadas agrupadas em duas funções, milhares de grupos, erros de sintaxe e fronteiras estruturais. Uma comparação independente de 11.240 fontes sintéticas nos caminhos script, módulo e Function realizou 33.720 comparações com a versão anterior, sem diferenças de AST, erro, offset ou aceitação nos casos rasos.

`fixtures/javascript-call-storage.html` combina 64 parâmetros/argumentos, chamada direta, apply/Reflect, bind, construção e agrupamentos alternados com chamadas. Ela deve renderizar `PASS: chamadas reservadas e grupos iterativos.`. Não contém capturas de sites nem constitui prova de compatibilidade completa com aplicações remotas.

## Resultados focais

No estado final deste recorte, passaram 686 testes da VM, incluindo 29 novos de listas de chamada e seis novos de grupos, e 197 testes de bindings. Passaram fmt, diff-check e Clippy com todos os targets e warnings como erros nos crates VM, bindings, renderer e CLI. A fixture combinada passou no CLI/renderização em subprocesso com pilha nativa de 1 MiB, saída 0, texto PASS e nenhum warning. A revisão independente encontrou e ajudou a corrigir a contagem de nomes var repetidos e uma cópia residual de argumentos em String boxed.

Esses resultados são sintéticos e focais. A entrega requer ainda a validação integrada do workspace e QA do checkpoint no Windows; não demonstram resolução de um site real.
