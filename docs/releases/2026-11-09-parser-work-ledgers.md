# Trabalho agregado de agrupamentos e inicializadores de for

## Contrato de admissão

O contador principal passa a se chamar `visits`, limitado a 16.384 entradas de gramática/validação. Conserva todas as categorias de cobrança anteriores exceto a entrada de agrupamento: statements, expressões-raiz, filhos Pratt, patterns, cláusulas switch, validação de conflitos lexicais e módulos. Não é uma contagem de todos os nós AST, bytes alocados ou operações de CPU. Por exemplo, members, chamadas vazias, holes e propriedades shorthand podem construir várias estruturas por entrada.

Grupos passam a um contador cumulativo próprio, `group_visits`, também limitado a 16.384. A escolha mantém a escala máxima anterior desse trabalho isolado, sem inventar um peso relativo entre operações heterogêneas. São possíveis até 32.768 entradas combinadas. Portanto a admissão muda: **não é o mesmo total de trabalho anterior** e não aumenta o teto de entradas principais para acomodar qualquer fonte específica.

Ambos os saldos são herdados e devolvidos por subparsers de templates, antes de propagar sucesso ou falha. Interpolações separadas e aninhadas compartilham o total. Parâmetros e corpo de Function usam o mesmo saldo, embora sejam gramáticas/lexagens distintas. Recuos de índice e tentativas que falham não repõem consumo. Os tetos do lexer são por lexagem: não limitam sozinhos o trabalho acumulado quando o mesmo texto é revisitado ou várias interpolações são analisadas.

As recusas de Resource têm diagnósticos numéricos específicos: `Parser visit budget exceeded` com `parser visits=16385 limit=16384`, ou `Parser group visit budget exceeded` com `parser group-visits=16385 limit=16384`. Não inventam offsets nem incluem fonte ou nomes privados. O teto é conferido na origem correspondente; uma raiz/filho que excede visitas não é substituída por uma falha de grupos posterior.

## Custo físico e limites preservados

O grupo usa o `usize` já existente no Context Pratt. Não acrescenta nó AST, slot de continuação ou frame nativo por parêntese. Seu processamento ainda consome CPU: abertura/fechamento, escolha de gramática e retomada da cauda. O contador agregado limita a O(G) os passos adicionais de controle dos agrupamentos, com G no máximo 16.384 por compilação; não mede sozinho o parsing ou a validação das caudas alteradas. Acrescenta-se um `usize` por estado Parser, não uma coleção proporcional aos grupos.

Grupos inalterados reaproveitam a validação existente por `checked`/`tail_start`; não revarrem uma árvore larga em cada fechamento. Uma cauda alterada continua passando pelo guard físico antes da composição seguinte. Sequências de vírgula conservam sua altura imutável verificada e quotas próprias. Não se afirma que uma visita de grupo tenha sempre menor tempo que qualquer visita principal; a vantagem física é não criar a estrutura/frame antes associado à recursão redundante.

Fonte de 128 KiB, slots/payloads de tokens, AST física 64, profundidade nativa compartilhada 64, contextos lógicos agregados 64 e cadeia local de 63 links permanecem. Sequence continua limitada a 4.096 operandos e 1.572.864 bytes cumulativos de capacidade/cabeçalho em x64. Nenhum contador da VM, autoridade do host ou fronteira de isolamento muda.

Um orçamento ponderado exigiria uma razão arbitrária entre visitas que já têm custos muito diferentes: literal, construção de função, despacho ou varredura de validação. Os dois contadores expõem unidades concretas e máximos próprios em vez de prometer uma equivalência de CPU ou memória que não existe.

## Parsing único de for

O primeiro Pattern de uma declaração é reutilizado por sua continuação de inicializador. Para referências, o mesmo Context/AST Pratt conclui o prefixo de precedência mínima 10 e, se não for binding de in/of, continua em 0. Não reanalisa nem realoca o prefixo descartado. O contador da cauda e a validação são preservados; a tentativa terminal na mesma posição não gera incremento fictício ao retomar. Yield direto conserva a seleção de gramática aplicável e for-await mantém suas restrições.

Trabalho que deixa de ocorrer deixa de ser debitado. Isso é diferente de devolver cobranças após um erro ou um probe: os saldos consumidos não são reduzidos. Não há mudança na validação de conflitos lexicais ou cobranças de módulo neste recorte.

## Verificação

Testes focais cobrem os limites principal/grupos exatos e combinados, diagnóstico da origem correta, templates separados/aninhados/falhos, Function em ambos os inputs, novas tentativas após Syntax sem reembolso, cobrança de patterns/filhos/validação, caudas alteradas e Clone/Drop/parsing com pilha de 1 MiB. A fixture usa somente fontes sintéticas.

Uma medição avulsa de parsing, sem histogramas por visita e sem acrescentar testes ignorados à suíte, excluiu geração da fonte, lexagem e destruição final do intervalo. Foram 25 amostras por forma, em Linux x64 debug, num ambiente compartilhado. Medianas locais: 16.000 grupos, 6,33 ms; 16.000 statements vazios, 1,38 ms; 8.000 statements `1;` (16.000 entradas principais), 10,06 ms; array de 8.000 itens, 6,78 ms; o mesmo array com 8.000 grupos, 11,03 ms. Grupos não são universalmente mais rápidos que uma entrada principal. A medição ilustra custo adicional limitado, não uma razão fixa de pesos. Não são limites universais de latência, memória ou RSS, e não há assert de tempo nos testes. Agregado de workspace e QA de sites reais são etapas independentes; estas mudanças não prometem que uma captura específica caiba nem removem a recusa estrutural de árvores genuinamente profundas.

## Resultado focal

Passaram 733 testes da VM, incluindo nove testes dos contadores e oito de parsing único de for. A integração focal passou em 197 testes de bindings, 16 do renderer e 35 do CLI. Clippy desses quatro crates com todos os targets e warnings como erros, fmt do workspace e diff-check passaram. O teste de diagnósticos dos bindings verifica os dois novos contadores, ausência de posição/fonte inventada e Resource fatal sem mutação do documento.

Um diferencial independente reconstruiu v8 e este recorte em targets separados, com binários distintos e controles positivos de admissão: 31.754 fontes sintéticas foram comparadas em script, módulo e Function, totalizando 95.262 pares com ASTs completas, erros e offsets idênticos. Dezenove controles de fronteira verificaram separadamente as admissões novas e as recusas preservadas. Ambas as versões executaram esse parsing em thread de 1 MiB; não é um diferencial de runtime ou de sites.

As fixtures `javascript-parser-work.html` e `javascript-parser-work-function.html` passaram no CLI em subprocessos com pilha de 1 MiB, com texto PASS e nenhum warning. A primeira combina 7.000 grupos com initializer de 5.000 statements e verifica efeito único; os mesmos bytes falharam no CLI v8 por exceder o contador anterior. A segunda cobre também o compartilhamento de grupos entre parâmetros e corpo de Function. A rodada completa do workspace e a validação Windows permanecem etapas separadas.

Na verificação focal conjunta, passaram 733 testes da VM, sem falhas ou ignorados, incluindo nove regressões dos dois contadores e oito do parsing único de for. Passou também Clippy de web-javascript com todos os targets e warnings como erros. Isso não substitui o agregado final do workspace ou a execução das fixtures no CLI.
