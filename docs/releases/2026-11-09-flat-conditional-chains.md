# Caudas condicionais planas

Este recorte representa espinhas contíguas do ramo falso (`no`) de expressões
condicionais como `ConditionalChain`. Uma expressão condicional isolada mantém
seus três filhos em `Box`. A mudança não cria uma arena geral de expressões.

## Construção e limites

- O parser Pratt consome as cláusulas de dentro para fora. O armazenamento segue
  essa ordem reversa, com inserção amortizada no final do vetor, e os consumidores
  percorrem as cláusulas na ordem do código fonte.
- Somente a aresta imediata `no` é compactada. Condições, ramos verdadeiros,
  funções, chamadas, arrays e sequências continuam sendo filhos independentes.
  Parênteses transparentes não introduzem um novo nó.
- O dono é um `Arc` imutável após publicação. A extensão exige posse exclusiva;
  um alias inesperado gera `Resource`, sem cópia implícita do sufixo.
- A capacidade cresce em aproximadamente 1,5 vez e tem teto local de 64 cláusulas.
  Isso não garante que uma fonte com 64 ternários diretos seja admitida: o guard
  lógico de Pratt continua ativo e pode recusar antes.
- `ConditionalChain`, `Sequence` e `IfChain` compartilham o mesmo ledger cumulativo
  de armazenamento. O teto é o menor entre a fórmula histórica por layout e
  1.572.864 bytes. Cabeçalho alinhado do `Arc` e capacidade real do vetor são
  cobrados antes da reserva; erros, templates e parâmetros/corpo de `Function`
  não reembolsam esse consumo.
- O builder não reparsa a fonte nem acrescenta visitas: as entradas Pratt já
  cobram cada parte da gramática.

Em x64, `Expr` permanece com 64 bytes e cada par condição/ramo verdadeiro ocupa
128 bytes. O cabeçalho alinhado com os dados do `Arc`, incluindo fallback e cache,
ocupa 112 bytes. A primeira cadeia de duas cláusulas cobra 368 bytes no ledger.
Esses números medem layout e capacidade, não metadados do alocador, e não implicam
uma redução universal de memória para qualquer programa.

## Altura residual e execução

O cache de altura é `1 + max(filhos)`, incluindo condições, ramos verdadeiros e
fallback. O cálculo considera corpos de funções, classes e defaults de parâmetros.
A extensão reaproveita o cache validado e inspeciona somente os dois novos filhos.
O cache é conferido ao compor a cadeia com outra expressão.

A compactação altera deliberadamente a altura física desta família. Uma cadeia
pronta pode entrar em outro contexto antes recusado pela representação recursiva.
Nenhum teto de fonte, lexer, visitas, grupos, pilha nativa, Pratt, encadeamento,
altura residual, avaliação, chamadas, statements ou VM foi aumentado.

A avaliação síncrona percorre a cadeia sem reconstruí-la. A entrada externa já
cobra o primeiro tick; cada condicional removido é cobrado somente quando seu
ramo falso é alcançado. Condições e ramos selecionados mantêm os checks de avaliação,
e a guarda de profundidade permanece ativa durante toda a operação.

A avaliação suspensível mantém um cursor e o mesmo dono da cadeia. Uma transição
falsa agenda um frame equivalente à antiga avaliação do próximo condicional,
preservando as fases de dispatch, tick de expressão, checks e suspensão. O cursor
não aumenta o encadeamento de continuations nem copia o restante da cadeia. Roots,
escopos e temporários externos são preservados entre suspensões e coleta de lixo.
`Resource` continua fatal, sem executar catch, finally, fechamento de iteradores
ou jobs posteriores.

O resultado continua sendo um valor, não uma referência: chamar um método obtido
por condicional não preserva o receiver, e selecionar `eval` não o transforma em
chamada direta. As regras de assignment, update, delete, typeof e modo estrito
continuam baseadas na forma da expressão. Os validadores visitam inclusive ramos
não selecionados, em ordem fonte, com trabalho virtual intercalado quando aplicável.

## Verificação

Os testes específicos exercitam árvores legadas pequenas como referência para
resultado, erro, orçamento restante e prefixo de efeitos em cada fase. Também
cobrem primeira, intermediária e última escolha, fallback, branches lazy, throws,
coerção, referências, eval indireto, gramática, offsets, ASI, armazenamento compartilhado,
altura residual, aliases, clone/drop e execução em thread com 1 MiB de pilha.
A validação de expressões e classes usa um walker iterativo de referências, com
cursores para irmãos. Não reconstrói o AST e não cria uma fila proporcional à
largura. `Function` continua sendo uma fronteira; statements e patterns mantêm
seus caminhos anteriores. A ordem DFS, flags de contexto, steps e limites de
validação permanecem iguais: 256 níveis de continuation e 65.536 unidades de
trabalho, distintos da altura residual de 64 admitida pelo parser.

A reserva temporária é falível e limitada a 773 slots: no máximo três irmãos
pendentes por nível (fallback, cursor da próxima cláusula e ramo verdadeiro),
mais espaço para a raiz e a primeira entrada que ultrapassa o limite. A capacidade
real é conferida após a reserva e não cresce. Em x64, cada item ocupa 32 bytes:
o payload máximo é 24.736 bytes, sem contar metadados do alocador ou RSS. Cursores
administrativos não cobram trabalho JavaScript. Sequence mantém sua pré-cobrança
histórica e ConditionalChain intercala o step de cada cláusula.

As regressões de 1 MiB incluem 60 níveis pelo ramo verdadeiro sob 30 declarações,
e 60 ou 62 declarações com folhas Binary ou Call de 60 níveis, exigindo Resource
sem abort. Outra regressão combina 15 níveis residuais com Binary, Call e Function
reais: parse, clone/drop e execução passam; a composição seguinte continua
recusada pela altura.

Os testes suspensíveis abrangem generator, async, async generator, yield/await,
GC, abrupt completions, contagem dos validadores e Resource antes/depois da suspensão.

A fixture `fixtures/javascript-conditional-chains.html` combina uma cadeia limitada
com operadores externos e verifica efeitos e suspensões. Esse teste sintético
não comprova compatibilidade geral com scripts de sites reais; alternâncias de
outras famílias de expressão continuam sujeitas aos limites existentes.

Verificação local final: 789 testes de `web-javascript` passaram, sem falhas ou
ignorados; Clippy do crate com todos os targets e warnings negados,
`cargo fmt --all --check` e `git diff --check` passaram. O oracle recursivo raso
confirma resultado, erro e trabalho do walker nos quatro kinds de função, nos
dois modos de contexto e nas fronteiras de profundidade e orçamento. O gate
integrado do workspace e o replay de páginas ficam separados desta verificação.

A integração Linux do recorte passou também 197 testes de bindings, 16 do renderer
e 35 do CLI. A fixture exibiu `PASS: caudas condicionais, efeitos e suspensao.`
sem warnings no CLI com pilha de 1 MiB. Os mesmos bytes eram recusados pelo
guard estrutural no checkpoint anterior.

Uma revisão independente comparou 524 ASTs/erros, 502 estados de contadores,
111.454 execuções e 26.216 retomadas com GC com o checkpoint anterior, sem
diferenças nos casos de paridade. Os controles de nova admissão ficaram separados.
A análise adicional do validador distingue os negativos recusados pelo parser:
508 dos 886 corpos dessa matriz chegaram ao helper de validação. Esses resultados
sintéticos não substituem o agregado final do workspace ou a verificação em Windows.
