# Avaliação síncrona com continuações limitadas

Estado: implementação integrada e revisão textual concluídas. Foram escritos o núcleo, os providers, os guards, as fixtures e as regressões do bloco completo antes de iniciar qualquer compilação ou teste. A validação focal e a comparação sintética independente passaram; o agregado workspace final está pendente. Os contratos abaixo não representam compatibilidade geral com sites.

## Arquitetura

O percurso comum de expressões usa uma máquina síncrona com referências emprestadas da AST e um vetor de continuações. Cada ancestral físico mantém no máximo um frame; uma nova fase substitui a anterior. Literais, argumentos, Sequence e ConditionalChain avançam por cursor, sem copiar árvores ou sufixos e sem encadear Actions por Box.

O dispatcher cobre folhas, operadores, curto-circuito, seleções, referências, atribuições, updates, delete/typeof, literais, preparação de calls/new e operandos de import. Place e PreparedCall continuam responsáveis pela referência e pelo alvo capturado. A chamada conserva callee e receiver antes de avaliar argumentos. Eval direto, private/super e LegacyMethod Host mantêm caminhos explícitos.

Funções e statements não viram uma máquina global sem pilha. Classes, padrões, invocação efetiva, eval dinâmico, iteradores, getters e coerções continuam usando helpers síncronos identificados. Esses bridges podem reentrar no dispatcher, compartilhando quotas e mantendo guards nativos. Não existe fallback genérico silencioso para o antigo eval_inner.

## Armazenamento e trabalho

O limite nativo de 64 entradas permanece, mas passa a contar dispatchers Rust reentrantes, não cada filho percorrido pelo loop. Chamadas 16, statements 64, coerção 8, código dinâmico 8 e scripts nested 8 permanecem independentes. A altura física de AST continua 64.

A tabela de fases usa um frame por nível físico e um slot de margem para cursor/raiz. A capacidade geométrica correspondente é 128. O envelope ordinário considera script raiz e até 16 chamadas; bridges têm teto nativo próprio e compartilham os limites globais. A contabilidade separa:

- Frames retidos no vetor: até 64 × 65, agregados entre dispatchers.
- Capacidade retida: até 64 × 128 slots, cobrada mesmo após pop enquanto o vetor existir.
- Bytes de frames: envelope de 17 × 128 × 512 bytes. Um assert impõe layout de frame de no máximo 512 bytes; a cobrança usa o layout compilado e a capacidade real. O layout ainda não foi medido nesta etapa.
- Trabalho administrativo: transições, push/pop e slots solicitados/movidos em crescimento. O saldo deriva do orçamento inicial de instruções multiplicado por 16, com teto absoluto de 32 milhões e aritmética verificada. Não substitui ticks semânticos nem é restituído por rollback.

Esses valores não são um teto de RSS, de toda a AST ou de toda a stack Rust. Frames movidos temporariamente para aplicação continuam sob a barreira ativa; buffers de argumentos, arrays, propriedades e strings mantêm seus budgets próprios. Metadados retidos em referências e alvos recebem reserva falível e cobrança cumulativa. Crescimento é limitado e verificado antes do efeito que precisa de uma continuação.

## Efeitos, GC e transações

Os ticks de entrada de expressão e os ticks virtuais de Sequence/ConditionalChain são preservados. Push/pop administrativos têm saldo separado. Curto-circuito não avalia ramos descartados. Referências não são resolvidas novamente, e GetValue/coerção/escrita mantêm sua ordem. Argumentos regulares reservam espaço antes da avaliação; spread continua incremental, com as regras existentes de IteratorClose e Resource.

GC e checkpoints de jobs continuam vedados durante todo o dispatcher, inclusive quando um frame já saiu do vetor, durante callback e no teardown. Resource permanece fatal: não executa finally, cleanup de página, conversões para diagnóstico ou continuação pendente.

Clones ativos são destinados somente a snapshots, não à execução independente. O contrato de rollback deve preservar as reservas dos dispatchers ainda vivos, identidade de origem e geração/contexto válido, sem restaurar snapshots ativos tardios ou de outra VM. Clones ociosos continuam independentes. Cada callback Host que recebe Runtime tem identidade de ativação própria, contador ativo limitado a 16 e verificação fatal na saída. Isso recusa snapshots usados em callbacks posteriores, inclusive dentro da mesma expressão, sem invalidar um rollback imediato após o retorno de filho nested. Uma recusa não substitui o heap incompatível e aborta a execução atual com Resource. Não há créditos compartilhados entre máquinas nem reposição de trabalho consumido.

## Inventário de bridges

| Caminho | Bridge mantido | Fronteira preservada |
| --- | --- | --- |
| Chamadas, new, bind e métodos Host | Invocação efetiva e ativação | Call depth, receiver, LegacyMethod e permissões Host |
| Eval direto/indireto e Function | Parser e execução de código dinâmico | Fonte, profundidade dinâmica, ambiente lexical e strict |
| Classes e padrões/defaults | Construção/binding já existentes | Guards de eval/call, escopo, privados, iteradores e rollback |
| Member, operadores e templates | Getters/setters e ToPrimitive/ToString | Ordem de efeito, receptor e guard de coerção |
| Arrays e argumentos com spread | Iteração incremental | Reserva de slots, largura, trabalho e fechamento existente |
| Object spread | Cópia de propriedades e getters/símbolos | Ordem, quotas de propriedades e atribuição própria |
| Async/generator | Representação suspensa atual chamando o núcleo síncrono | Roots persistentes, suspensão, Resource e budgets compartilhados |
| Import | Resolução após avaliar operandos | Recursos já autorizados, falha síncrona dos operandos e Promise |

Não amplia a gramática: computed keys de object literals, sintaxe de atribuição lógica e super com símbolo permanecem fora do subconjunto atual. Não corrige automaticamente as limitações anteriores de Symbol.hasInstance, de super com accessor próprio no receiver ou de F.prototype nativo. Não adiciona I/O ao motor, engine externo ou permissões de navegação.

## Fechamento planejado

As regressões escritas cobrem paridade de valores/ticks/efeitos, reservas e exhaustion, composição entre chamadas, fases de referências, direct eval, spread, literal builders, GC/job barriers, clones/snapshots e rollback. A fixture javascript-iterative-evaluation.html combina uma expressão rasa com 13 chamadas limitadas, ordem de coerção/atribuição, callee capturado, literais, templates, new, eval e bridges de async/generator. Os testes DOM incluem pilha de 1 MiB e rollback fatal sem cleanup.

Após o bloco completo: revisão integrada, fmt, Clippy, suíte workspace, fixtures e checagem de stack/Windows sobre a mesma fonte. Resultados executados serão registrados separadamente dos testes apenas escritos. A melhora estrutural não prova funcionamento de Google ou de qualquer aplicação Web completa. Nenhuma publicação faz parte desta fase.

## Validação focal executada

A implementação completa e a revisão textual precederam a primeira compilação. A rodada inicial encontrou uma divergência na ordem de microtasks após finalizadores e uma regressão nova cuja fonte era recusada pelo parser antes de alcançar o executor. A correção manteve a ordem esperada dos jobs e o limite do parser. O checkpoint interno após cada finalizador admite apenas o marcador do loop externo de finalização; não admite frames de página vivos nem drenagem reentrante de Promises.

A repetição integral de web-javascript passou 850 testes. A integração de web-bindings, web-renderer e browser-cli passou outros 252 testes, incluindo execução DOM em thread de 1 MiB e rollback fatal sem cleanup. Nenhum teste desses grupos foi ignorado. Fmt, Clippy workspace/all-targets com warnings tratados como erros e git diff --check passaram. Ajustes de estilo exigidos pelo Clippy não alteraram o contrato.

O CLI foi recompilado e executou as 14 fixtures do pacote, incluindo javascript-iterative-evaluation.html, sob limite de stack de 1 MiB: todas produziram o texto esperado, sem warnings ou marcador de execução pendente. Esse resultado Linux não substitui a verificação do binário Windows. O agregado workspace final ainda está pendente nesta etapa; não se declara compatibilidade completa com sites.

## Comparação independente

O corpus externo reconstruiu a V12 a partir de sua base e do checkpoint privado validado, em vez de usar somente o HEAD público. Foram comparadas 9.484 observações por versão, cobrindo valor, erro, valor lançado, instruções restantes e efeitos Host. Não houve divergência inesperada no subconjunto comum nem falha nas expectativas. O caso de sequência direta recusado pelo parser permanece controle negativo; a forma agrupada de 1.000 itens exercita o cursor largo sem ampliar a gramática.

Quatro composições finitas antes recusadas por profundidade cumulativa de avaliação completaram na V13, incluindo chamadas com ancestrais binários e literais aninhados. São mudanças de admissão esperadas, não equivalência com o erro antigo. Outros 16 cenários públicos verificaram os novos contratos de snapshots e callbacks somente na V13. Todos os resultados foram reproduzidos com stack limitada a 1 MiB, sem diferenças nas observações.

Essa comparação é sintética e limitada ao corpus. Não substitui a suíte completa, a QA Windows ou a execução real de sites, e não demonstra paridade universal de JavaScript.
