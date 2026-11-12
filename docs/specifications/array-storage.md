# Armazenamento limitado de arrays

Arrays continuam usando `Vec<Value>` para os valores. Esta mudança não implementa arrays esparsos com comprimento arbitrário: o comprimento máximo é 8192, centralizado em `MAX_ARRAY_LENGTH`. A ausência de uma propriedade indexada (hole) usa um bitmap de `u64`, em vez de uma entrada de árvore por índice ausente. Um hole continua distinto de uma propriedade presente contendo `undefined`, inclusive em leitura herdada, `in`, `Object.hasOwn`, delete, length e métodos.

## Orçamento e custo

- No máximo 8192 posições por array, incluindo índices e expansão de length. Construtor, setters, Array.from e resultados dos métodos de Array aplicam o mesmo teto.
- Permanecem 65.536 reservas cumulativas de posições por VM. Aumentar o comprimento de um único array não aumenta o teto agregado. Alocações e expansões debitam uma instrução por posição, além do trabalho dos métodos.
- A verificação de posições e trabalho precede a alocação do backing do construtor e sua expansão. Resultados de tamanho conhecido reservam posições antes de copiar valores ou executar getters/callbacks. Concat reserva cada segmento; filter/flat reservam cada posição quando o resultado é conhecido, antes de adicionar ao buffer. Array.from iterável reserva seu destino interno antes do mapper, sem cobrar novamente no write final. Falhas de recurso continuam fatais. GC, pop, delete, truncamento e rollback não reembolsam as reservas cumulativas.
- O bitmap cresce geometricamente com teto de 128 palavras, isto é, 1024 bytes de payload por array. Intervalos de holes são marcados por palavras, sem construir milhares de nós de árvore. Falhas de reserva do bitmap são Resource.
- O vetor de valores continua denso e pode ter capacidade maior que seu comprimento. Bitmap, capacidade do vetor, mapas, payloads referenciados, clones transacionais e alocador têm custos adicionais. O limite de posições e 1024 bytes de bitmap não são uma promessa de RSS total.

## Limites independentes

Não mudam os limites de tokens, AST, profundidade, strings, heap, scopes, chamadas, instruções padrão, typed arrays, nem as quotas próprias de iteradores genéricos e enumeração. Literais e expansão/rest continuam sujeitos às suas quotas de 4096 itens. Métodos que visitam todo um array maior podem consumir o orçamento de instruções antes de terminar; esse erro não é ignorado. Enumeração de muitos índices pode atingir sua quota de chaves mesmo quando o comprimento do array é permitido.

A integração não cria I/O ou autoridade do host, não muda IPC ou sandbox e não constitui prova de compatibilidade completa de sites.

## Cobertura

forEach não constrói um vetor de resultados descartados. As reservas já feitas permanecem consumidas se um callback falhar.

Os testes comparam o bitmap a um conjunto de referência nas fronteiras de palavras e comprimento; verificam capacidade, rejeição sem mutação, holes/undefined/herança, length, GC, rollback, métodos, preflight e orçamento acumulado. O teste que combina várias operações lineares usa orçamento explícito do host; outro teste mantém a recusa por trabalho insuficiente. A fixture `javascript-array-storage.html` exercita o construtor, holes e join num documento local.


## Listas variáveis de chamadas

O recorte de [chamadas reservadas](../releases/2026-11-09-bounded-call-lists-and-parser-groups.md) também usa as mesmas 65.536 células cumulativas para capacidade solicitada de buffers variáveis de argumentos e suas cópias reais. Não há um segundo teto que amplie esse envelope. Mover armazenamento já reservado não cobra uma cópia; GC e rollback não repõem reservas. Arrays e chamadas repetidas competem pelo mesmo saldo, podendo esgotá-lo antes do comportamento antigo com temporários não contabilizados.
