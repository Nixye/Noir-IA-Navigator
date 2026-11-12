# Continuações Pratt limitadas

Nota de evolução: esta etapa preservava a cobrança conjunta de grupos e entradas do parser. O recorte posterior de [contadores de trabalho separados](2026-11-09-parser-work-ledgers.md) mantém 16.384 entradas principais e acrescenta um teto agregado próprio de 16.384 grupos, permitindo até 32.768 entradas combinadas. Os demais guards descritos aqui continuam em vigor.

O parser deixa de manter um frame nativo por argumento de chamada, índice, prefixo, RHS binário/assignment ou ramo condicional. Um dispatcher itera pelos estados de início, átomo, cauda e retorno, com continuações explícitas. Parênteses iniciais continuam compactados num contador por contexto; listas de parâmetros de arrows continuam reconhecidas pelos delimitadores ligados do lexer.

## Limites independentes

- O guard nativo compartilhado permanece 64. Funções, statements, patterns, classes, constructors e outros átomos complexos não zeram essa profundidade ao reentrar no parser.
- Há no máximo 64 contextos lógicos de expressão ativos, somando raízes e continuações de todos os dispatchers reentrantes. Um subparser de template herda o saldo atual. A conferência precede a reserva de um slot de continuação.
- O saldo lógico acompanha trabalho ativo, não visitas cumulativas. Um filho concluído devolve seu slot; no erro, os parciais e a pilha local são descartados antes de restaurar o saldo de entrada. O contador de visitas nunca é devolvido: cada entrada lógica e cada grupo conservam a cobrança anterior, inclusive em probes e templates que falham.
- Fonte, tokens e o teto de 16.384 visitas permanecem inalterados. A AST recursiva continua sujeita a 64 níveis de expressão, incluindo corpos de funções, parâmetros e campos. O dispatcher isoladamente não muda a representação da AST; o recorte separado de [sequências de vírgula](2026-11-09-flat-comma-sequences.md) acrescenta armazenamento plano com quotas próprias.

A separação pode aceitar árvores que já cabiam em 64 níveis de AST, mas antes consumiam simultaneamente frames nativos de statements e chamadas. Não permite árvores mais profundas. Cadeias de chamadas genuinamente profundas continuam limitadas, e esta mudança isolada não demonstra compatibilidade com um site remoto.

## Capacidade retida das continuações

Os 64 contextos limitam conteúdo lógico ativo, não a soma das capacidades de todos os vetores. Um dispatcher pode esvaziar seu vetor e manter a capacidade de um pico anterior enquanto entra num átomo complexo. Para o dispatcher no nível nativo de expressões `d`, todos os seus ancestrais já mantêm pelo menos um contexto lógico: seu pico local nunca excede `64-d`. O crescimento do Vec no alvo, com mínimo quatro slots e duplicação, limita a capacidade retida por nível a zero ou à próxima potência de dois, com mínimo quatro. Somando os máximos independentes dos 64 níveis: `31×64 + 16×32 + 8×16 + 4×8 + 4×4 = 2.672 slots`. Isso já contempla picos anteriores, não apenas a ocupação atual.

A regressão reproduz a reserva incremental real de todos esses picos, confere capacidade máxima 64 por vetor e conserva a conferência após `clear`. No alvo x64 Linux com Rust 1.99, `Context` ocupa 32 bytes, `Continuation` 160 e `State` 104. O limite conservador é 427.520 bytes de payload de continuações, mais até 1.536 bytes de cabeçalhos Vec. Não inclui ASTs, buffers de argumentos, strings, tokens, overhead do allocator nem temporários da pilha compilada. Não é uma estimativa de RSS. Os testes de pilha de 1 MiB verificam separadamente o código debug real; `size_of(State)` não representa todo o frame nativo do dispatcher.

## Semântica e descarte parcial

Cada contexto preserva precedência mínima, grupos restantes e contador de sua cauda. O contador de cadeia inclui a tentativa terminal sem operador: uma cauda aceita 63 links. Retornar de um filho não reinicia esse contador; fechar um grupo inicia uma nova cauda. Assignments e exponenciação usam a mesma precedência para o RHS; os outros binários usam precedência mais um. A AST é montada na mesma ordem, sem reassociar operações, extrair métodos ou perder receivers. O builder binário fica separado do agendamento de filhos.

Cada expressão lógica concluída é validada antes de voltar à sua continuação. Caudas de grupos alteradas também são validadas antes da próxima composição; grupos inalterados não voltam a percorrer a mesma árvore. Um erro de sintaxe ainda pode descartar uma árvore parcial que combina um filho validado e uma cauda local limitada, como já ocorria antes. Não se acumula uma cadeia ilimitada para validá-la somente no retorno externo.

As guardas nativas de statements, patterns e constructors conferem o teto antes de incrementar a profundidade. Isso corrige um saldo residual após a recusa, sem mudar o teto, a aceitação ou a mensagem de erro; o estado de entrada volta a ser restaurado também nesses caminhos de Resource.

Yield sem argumento conclui apenas a expressão interna; uma cauda após fechar um grupo continua possível. ASI, arrows, alvos de update/assignment, spread, ordem de validação de argumentos e erros de delimitadores mantêm o comportamento anterior. A preparação de prefixos termina antes da entrada num átomo nativo, para não carregar seus temporários por cada função/classe/array reentrante em builds debug.

## Verificação

Os testes focais cobrem compartilhamento de saldo entre funções, defaults/patterns, arrays, objetos, classes, constructors e templates; restauração em sucesso e erro; Resource antes de analisar ou reservar filhos; visitas cumulativas; precedência, associação, receivers, ASI, yield e arrows; cadeias locais; Clone/Drop e falhas parciais em threads de 1 MiB. O agregado do workspace e QA de sites reais são etapas independentes.

No recorte do dispatcher, passaram 696 testes da VM (80 de parser, dos quais dez novos), Clippy da VM com todos os targets e warnings como erros, fmt do workspace e diff-check. Um diferencial independente de 994 fontes sintéticas comparou com v7 os resultados completos de parsing de módulo e execução de script, incluindo erros e offsets: 1.988 resultados idênticos. Esse diferencial não compara a serialização interna da AST nem substitui testes de módulos/Function, guardas estruturais ou QA real.

Uma segunda revisão, isolada antes da inclusão de Sequence, comparou 32.322 fontes geradas contra v7: 64.644 resultados de script/módulo, com ASTs completas e erros/offsets idênticos. A regressão posterior de restauração das três guardas nativas mantém os mesmos pontos de recusa, corrigindo somente o contador interno após erro.
