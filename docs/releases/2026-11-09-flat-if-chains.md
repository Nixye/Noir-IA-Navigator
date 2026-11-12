# Cadeias limitadas de `else if`

Cadeias de pelo menos duas condições consecutivas `if (...) ... else if (...) ...`
passam a usar `Stmt::IfChain`: um vetor de pares condição/corpo, um `else` opcional
e um dono `Arc` imutável. O parser percorre as arestas imediatas `else if` em um
laço. Não reconstrói uma cauda recursiva de `Box<Stmt>` depois do parse.

Blocos, labels, corpos `then`, `else` final, funções e outras gramáticas conservam
sua estrutura e seus guardas. Não há fusão através de chaves, escopos ou labels.
Um `if` isolado conserva a representação anterior. `Clone` de uma cadeia compartilha
o dono; os acessores expõem apenas referências imutáveis, sem copy-on-write.

## Limites e armazenamento

- Até 4.096 cláusulas por cadeia, crescimento de capacidade de aproximadamente 1,5×.
- O teto cumulativo que antes cobria `Sequence` agora é compartilhado por
  `Sequence` e `IfChain`, sem outro orçamento de armazenamento plano.
- O teto continua em 1.572.864 bytes no layout x64 suportado. Capacidade de `Vec`
  e layout alinhado do cabeçalho `Arc` são cobrados antes da reserva. Capacidade
  adicional eventualmente retornada pelo alocador também é cobrada.
- Falhas não devolvem o saldo. Interpolações de template herdam e devolvem o saldo
  consumido, inclusive em erro; parâmetros e corpo de `Function` usam o mesmo saldo.
- Combinações grandes de sequências e cadeias podem atingir o teto compartilhado,
  mesmo quando cada componente isoladamente caberia.
- Permanecem os limites de fonte de 128 KiB, 16.384 visitas principais, 16.384
  visitas de grupos, profundidade lógica Pratt de 64, profundidade nativa do parser de 64,
  árvore física de expressões de 64 e profundidade síncrona de statements de 64.

Layout medido no build Linux x64: `Expr` de 64 bytes, `Stmt` de 144 bytes, par de
cláusula de 208 bytes, cabeçalho/dados fixos de 184 bytes. Uma cadeia mínima com capacidade 2
cobra 600 bytes. A sequência mínima continua cobrando os mesmos 240 bytes anteriores
(cabeçalho de 48 + três expressões de 64). Isto não é uma promessa de redução universal de
RSS: cadeias pequenas e folga de capacidade podem custar mais que as antigas Boxes;
metadados/classes de tamanho do alocador não fazem parte dessa contabilidade.

## Execução, validação e coleta

A execução síncrona percorre condições em ordem e cobra o statement de cada `if`
virtual somente quando essa condição é alcançada. A máquina async/generator mantém
um dono compartilhado e um índice, preservando as fases e ticks de frames/expressões
sem clonar ou reconstruir sufixos. O cursor conserva as raízes de escopo em suspensões.

Hoisting conserva a ordem DFS e os ticks intercalados dos `if`s removidos com
marcadores limitados; não pré-debita toda a cadeia. A busca de `var` e o validador de
continuations preservam contagens e saída antecipada. Validações estritas, de escopos
e de módulos percorrem condições, corpos e fallback sem atravessar indevidamente
funções aninhadas. Condições e corpos retidos continuam sujeitos aos guardas existentes.

## Verificação sintética

Os testes comparam a forma plana com árvores pequenas de `Stmt::If` montadas
manualmente: primeiro/meio/último/nenhum ramo, resultados, erros, saldo de instruções,
efeitos, hoisting, detecção de `var`, precedência de validação, escopos, dangling else,
ASI, loops/labels e conclusões abruptas. Cobrem async/generator, coleta entre
suspensões e `Resource` fatal sem execução de catch/finally.

Testes explícitos em threads de 1 MiB cobrem parsing, clone, drop e execução de
cadeias largas, misturas com profundidade residual e recusas nos limites. Os testes
de armazenamento verificam capacidades, largura, teto compartilhado e saldo em erros,
templates e parâmetros dinâmicos. Estes são resultados sintéticos do subconjunto;
não demonstram compatibilidade completa com JavaScript nem com sites específicos.

Verificação Linux do recorte final: 760 testes da VM, 197 de bindings, 16 do renderer
e 35 do CLI passaram. A fixture `javascript-if-chains.html`, com 180 condições,
escopos separados, dangling else, generator e async, exibiu
`PASS: cadeias else-if, escopos e suspensao.` sem warnings no CLI com pilha de 1 MiB.
A mesma fixture no checkpoint anterior era recusada pelo guard nativo do parser.

Uma comparação independente com o checkpoint anterior encontrou igualdade em
249 parses normalizados e 35.574 execuções sintéticas nos casos dentro dos limites
anteriores, incluindo erros/offsets, resultados, efeitos e saldo de instruções.
Dezoito controles separados verificaram a nova admissão e os limites residuais e
compartilhados. Isso não substitui o agregado do workspace nem o QA em Windows.
