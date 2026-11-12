# Sequências de vírgula planas sob quotas independentes

## Representação e semântica

A partir de três operandos, uma cadeia associada à esquerda de operadores de vírgula passa de `Binary` encaixados para uma `Sequence` com filhos em vetor. Dois operandos conservam a representação anterior. Somente a cadeia esquerda é incorporada: um operando direito parentizado continua sendo a sua própria árvore. Operadores aritméticos, condicionais, assignments, calls e members não são reassociados.

O vetor é compartilhado por `Arc`, com campos privados e acesso somente por referências imutáveis. O parser move seus prefixos durante a construção. Se um prefixo estiver inesperadamente compartilhado, o construtor recusa a extensão, em vez de fazer uma cópia oculta. Clone da sequência compartilha o armazenamento; Drop percorre seus filhos em largura. A altura física é calculada na construção, atualizada apenas com a altura do novo operando direito e reaproveitada pelos guards. O cálculo inclui os níveis de funções, parâmetros e declarações de classe vazias que o walker anterior contava. Nenhum consumidor recebe acesso mutável aos filhos ou ao Arc.

Essa altura evita percorrer novamente todo o prefixo em cada grupo alterado de uma fonte como `(((a,b),c),d)`. Os operandos continuam avaliados uma única vez, em ordem, e o resultado da vírgula continua sendo um Value, não uma Reference. Chamadas indiretas de eval, perda de receiver, delete de expressões e diretivas estritas mantêm suas fronteiras. Erro ou suspensão não executa antecipadamente os operandos seguintes.

## Armazenamento e trabalho

Cada sequência aceita no máximo 4.096 operandos. A capacidade parte de três e cresce aproximadamente 1,5 vez, com `try_reserve_exact` e teto explícito. Um contador cumulativo independente cobra a capacidade adicional e o layout equivalente do Arc antes da reserva. Se a Vec reportar capacidade maior que a planejada, essa diferença é contabilizada ou recusada. Falhas, probes de parsing e interpolações de templates não devolvem consumo.

O orçamento é derivado de 16.384 × (`size_of::<Expr>()` + metade desse tamanho). Não afirma que as visitas existentes do parser contem todos os nós ou todas as alocações. No alvo x64 medido, Expr ocupa 64 bytes, o layout do Arc com vetor e altura custa 48 bytes e o orçamento é 1.572.864 bytes, incluindo ambos. A sequência inicial custa 240 bytes solicitados de payload, contra 258 dos quatro Boxes e dois textos de vírgula substituídos. Isso não demonstra economia universal de memória.

A capacidade retida/cumulativamente reservada é distinta do pico de realocação: uma expansão pode manter temporariamente o buffer antigo junto do novo. O antigo buffer de uma única sequência é limitado a 4.096 × `size_of::<Expr>()`. Cabeçalhos e alinhamento são derivados do layout; metadados invisíveis e classes de tamanho do allocator não fazem parte dessa promessa. Payloads dos filhos e outras estruturas conservam seus próprios limites.

Os nós binários removidos não tornam o trabalho gratuito. A execução síncrona pré-debita um tick por ancestral removido, antes do primeiro operando. O caminho suspensível pré-debita dois por ancestral, correspondentes ao dispatcher e à entrada de expressão. Um cursor agenda um Apply para cada operando não final; o último usa a continuação original diretamente. Isso preserva saldos e a fase da falha por instruções em comparação com a AST Binary equivalente. A validação de continuações também cobra as visitas lógicas removidas.

O cursor conserva a continuação externa e o tracing de seus valores, receivers e lugares. Ele não retém um acumulador extra: o caminho de vírgula suspensível anterior já descartava o valor esquerdo antes do próximo operando.

## Limites preservados e verificação

Continuam em vigor os limites de fonte, tokens, visitas de parsing, cadeia local de 63 links, profundidade física AST de 64, guards nativos e quotas da VM. A sequência não habilita novas posições de await/yield nem amplia o limite de argumentos. Resource permanece fatal, sem executar catch/finally da página.

Testes específicos comparam Sequence com AST Binary construída manualmente: resultados, erros, efeitos já executados, saldo de instruções e trabalho do validador. Cobrem script, módulo e Function, limites de largura/capacidade, templates e probes sem reembolso, classes vazias, aliases imutáveis, receiver/eval/delete/diretivas, await/yield nos três tipos de função, retorno/throw e coleta entre suspensões. Parsing, Clone/Drop e execução de grupos longos são exercitados em threads de 1 MiB.

Esses testes são sintéticos. A mudança não comprova compatibilidade completa com sites remotos nem resolução de um fluxo externo específico. Resultados integrados de workspace e da fixture são registrados separadamente na entrega.

## Resultado focal desta implementação

Passaram 716 testes da VM, incluindo 19 testes novos específicos de Sequence. Passaram também `cargo clippy -p web-javascript --all-targets --locked -- -D warnings`, `cargo fmt --all --check` e `git diff --check`. Os testes de erro por orçamento comparam a representação plana e a AST Binary manual em vários saldos antes e depois de suspensões. Há cobertura de Clone/Drop tanto para 4.096 operandos quanto para sequências aninhadas até altura física 64, em pilha de 1 MiB. Isso não substitui a rodada integrada do workspace nem QA no alvo de produção.

A revisão independente comparou 7.250 execuções sintéticas (250 fontes e 29 saldos de instruções) com a rlib anterior sem Sequence, sem divergências de resultado, saldo ou efeitos observados. A integração focal passou em 197 testes de bindings, 16 do renderer e 35 do CLI, além de Clippy desses crates e da VM. A fixture `fixtures/javascript-expression-storage.html` passou no build final do CLI em subprocesso com pilha de 1 MiB, com texto PASS e lista de warnings vazia. Ela respeita os limites independentes de cadeia local e avaliação; não contém dados de capturas privadas.
