# Concatenação e eval com reservas limitadas

## Escopo

O operador `+` quando produz string e `String.concat` admitem até 128 KiB na representação final: bytes UTF-8 quando bem formada, ou duas vezes a quantidade de unidades UTF-16 quando há surrogate isolado. Os demais produtores continuam sujeitos aos seus tetos próprios, inclusive 64 KiB em `repeat`, joins e templates. Não há promessa de suporte irrestrito a strings grandes em todos os métodos.

A concatenação de UTF-8 usa os operandos imutáveis diretamente. Para UTF-16, um iterador emprestado mede o resultado sem criar vetores dos operandos; a decodificação atravessa a fronteira entre eles, preservando pares que se formam nessa junção. Só depois da medição o resultado é alocado. O orçamento cumulativo reserva duas vezes os bytes finais para o buffer e o payload Arc. É uma reserva conservadora de payload, não uma medida exata de RSS, metadados ou comportamento do alocador. Não foram adicionadas ropes ou armazenamento compartilhado de subintervalos.

Varreduras e cópias consomem uma unidade de trabalho mais uma por bloco de 64 bytes de entrada. O orçamento de instruções padrão permanece 100.000, e o limite cumulativo de strings permanece 4 MiB. Reservas não são devolvidas por rollback, GC ou falhas posteriores. `.length` e índices não materializam uma cópia UTF-16 completa; lookup de métodos e constructor não percorre o conteúdo.

`String.concat` converte receiver e argumentos com ToString observável, na ordem original, uma vez por argumento. O operador `+` mantém a ordem e o hint de ToPrimitive existentes. Symbol continua sendo TypeError na conversão implícita. Exaustão de recursos continua fatal; diagnóstico contém somente operação e tamanhos numéricos.

## Fontes dinâmicas

Eval admite até 128 KiB, igual ao teto de entrada do parser. UTF-8 é emprestado de sua representação imutável; UTF-16 é medido e reservado antes de sua conversão. O total de fontes dinâmicas permanece 256 KiB por VM, inclusive tentativas que falham no parse, e o aninhamento permanece oito. Function mantém 64 KiB para parâmetros, corpo e separadores. Não muda o parser separado de parâmetros/corpo nem o escopo global de Function.

Tokens (32.768), nós sintáticos (16.384), profundidade, escopos, bindings, instruções, regex e JSON conservam suas quotas independentes. APIs que usam coerce_to_string continuam limitadas a 64 KiB; operações DOM conservam seus próprios limites de mutação, sem impor um teto global novo a todas as interfaces. Portanto uma fonte que agora cabe na concatenação e em eval ainda pode ser recusada pelo parser ou pela execução. Código remoto continua sujeito ao broker e à sandbox existentes.

## Verificação

Regressões sintéticas cobrem fronteiras de bytes, Unicode e surrogates, orçamento antes da montagem, rollback/GC, coerções observáveis, erros fatais e diagnósticos sem conteúdo. Eval é testado com fontes grandes, limite cumulativo, erros de tokens/sintaxe sem alteração de escopo, colisões lexicais e distinção direct/indirect/strict. O teste de bindings comprova atualização real do DOM, rollback e desativação após violação da fronteira host.

`fixtures/javascript-string-storage.html` demonstra concatenação e eval maiores que 64 KiB, publicando apenas um resultado curto no DOM. Testes sintéticos não demonstram funcionamento de sites reais.
