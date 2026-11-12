# Lexer compacto com orçamentos separados

## Escopo

Pontuação e operadores do lexer usam referências a strings estáticas. O payload raro de RegExp fica em uma caixa, sujeita ao limite existente de 64 regexes, e os tokens não são clonáveis. Em builds x64 de Windows e Linux, `Kind` ocupa 32 bytes e `Token` ocupa 48, em vez de 48 e 64. Posições em bytes, flags de quebra de linha e a distinção entre divisão e regex são preservadas.

O lexer admite até 40.960 tokens mais o marcador EOF em x64. Antes de ampliar o vetor, verifica a capacidade solicitada multiplicada pelo tamanho do token, mais uma reserva para todas as 64 caixas de RegExp. O máximo é 1.969.200 bytes, abaixo dos 2.097.216 bytes dos slots anteriores. Em outro layout, o limite de slots é reduzido se necessário para respeitar a mesma fórmula conservadora de armazenamento antigo.

Há um segundo limite de 32.768 tokens que carregam payload ou empilham contexto: Name, Number, BigInt, String, Template, RegExp e os símbolos `(`, `{`, `?`. A cobrança ocorre antes de atualizar os vetores de contexto. Só pontuação estática sem push pode usar os slots adicionais. O EOF não consome essas contagens. Se ele exige ampliar o vetor, é reservado somente um slot final, sem crescimento geométrico inútil. O diagnóstico informa apenas contagens e tamanhos, sem conteúdo da fonte.

A guarda de bytes mede metadados solicitados do vetor e das caixas, não RSS, cabeçalhos do alocador, cópias transitórias durante realocação, payloads de strings ou toda a AST. Pequenos programas com muitas regexes podem usar mais bytes por causa das caixas. Não há promessa de melhoria universal de memória ou tempo.

## Agrupamentos e trabalho do parser

Uma passagem linear liga parênteses usando um índice de 32 bits no padding já existente de cada token x64. Não usa vetor auxiliar nem recursão. O parser reconhece uma lista de parâmetros de arrow somente quando o parêntese correspondente é seguido por `=>` sem quebra de linha. Expressões agrupadas deixam de construir e descartar padrões especulativos.

Isso permite, por exemplo, uma expressão agrupada de vírgula com 34 nomes. Não aumenta o limite de funções ou arrows reais, que continua sendo 32 parâmetros. Defaults, desestruturação, rest, comentários e regras de quebra de linha têm regressões específicas. Visitas consumidas por interpolações de template são preservadas também quando a interpolação falha.

O contador existente de 16.384 mede visitas nas entradas principais do parser, não todos os objetos Expr/Stmt/Pattern construídos. Não é apresentado como teto completo de bytes ou nós da AST. O limite léxico é por lexagem, inclusive nas interpolações separadas de templates; não é um contador agregado de todos os tokens expandidos. Fonte de 128 KiB, limites de literais/regex, profundidade estrutural de 64 e quotas da VM continuam independentes e podem interromper antes. Não foi introduzido um teto novo de construções de AST neste recorte.

## Entradas de módulos

O limite existente de 256 entradas em cada coleção de imports, exports, requests únicas e star exports passa a ser conferido durante o parsing, antes de inserir ou clonar o specifier. Listas named/default/namespace e reexports compartilham os mesmos contadores cumulativos do módulo. Isso evita materializar milhares de cópias de um specifier antes de uma rejeição que já ocorreria no loader. Nenhum módulo antes elegível perde entradas silenciosamente.

Uma fonte já acima do limite pode agora produzir Resource antes de um erro de sintaxe ou duplicata que só apareceria depois. Resource continua fatal. Não muda resolução, rede, autoridade do host, IPC ou sandbox.

## Verificação

Regressões cobrem slots físicos e EOF, capacidade e reserva de bytes, payload/contexto 32.768/32.769, regexes 64/65, precedência lexical de erro, diagnósticos sem conteúdo, grupos e arrows, fonte, visitas, profundidade e execução em uma thread de 1 MiB. Um programa sintético de 37.485 tokens verifica parsing além do teto anterior. Entradas de módulos são testadas nos limites 256/257, inclusive com specifier longo e várias declarações cumulativas.

`fixtures/javascript-compact-lexer.html` contém 1.000 atribuições de objetos com oito propriedades cada. Sua fonte supera 32.768 tokens, cabe nos limites de payload e execução, e precisa atualizar o DOM com `PASS: lexer compacto limitado.`. O mesmo corpo é analisado pelos caminhos de script, módulo e Function. A fixture é sintética, não contém captura de site e não demonstra compatibilidade completa com sites reais.

## Resultados focais

No Linux x64, os 651 testes da VM e os 196 testes de bindings passaram. Clippy de ambos os crates, incluindo todos os targets e com warnings tratados como erros, passou. Os limites do parser também foram revisados independentemente. A fixture também passou pelo CLI/renderização em subprocesso sob pilha nativa de 1 MiB, com saída 0, texto PASS e nenhum warning. Esses resultados focais não substituem a validação integrada do workspace nem QA no Windows.

Uma comparação externa, somente com entradas sintéticas, usou builds release e instrumentação do alocador System fora do código de produção. Foram medidos bytes solicitados vivos/no pico, não RSS ou overhead físico do alocador. Dezesseis fingerprints de AST de entradas anteriormente válidas coincidiram. Em três repetições, o corpus minificado teve pico léxico 25,5% menor e pico de parsing 11,9% menor; 2.000 grupos simples reduziram as chamadas de alocação de 40.003 para 16.003. Entradas pequenas com regexes apresentaram aumentos de memória ou tempo, e timings em máquina compartilhada variaram. Esses números não são garantia de desempenho em sites reais.
