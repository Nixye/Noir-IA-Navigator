# Roadmap: coerção de operadores do item 1

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

Este incremento amplia a semântica do motor próprio. O item 1 continua aberto.

## Implementado

- Conversão falível de objetos para primitivas nos operadores. Symbol.toPrimitive recebe o hint apropriado, exige método chamável e resultado primitivo. Sem esse método, valueOf/toString são consultados na ordem correspondente ao hint.
- Soma, operações numéricas/bitwise, comparações, igualdade abstrata, operadores unários numéricos e atualização usam essas conversões. Wrappers BigInt e métodos sobrescritos participam do mesmo caminho.
- Operandos e referências de atribuição são avaliados uma vez. Conversões observáveis conservam a ordem, e erros de getters/métodos propagam sem repetir efeitos. Erros de recursos permanecem fatais.
- Identidade, igualdade estrita, truthiness e typeof não convertem o objeto. Atualização postfix devolve o valor numérico convertido anterior, não o wrapper original.
- Array.join converte separador e elementos com hint string, inclusive quando chamado durante a conversão de um array por operador.
- ToPropertyKey compartilhada converte chaves computadas com hint string e preserva Symbol. Get/call, referências de atribuição, atualização, delete, in e APIs Object/Reflect com chave usam o mesmo caminho, sem repetir getters ou conversões. In valida o objeto da direita antes de converter a chave. As diferentes ordens de conversão de Object.hasOwn e métodos de Object.prototype foram preservadas.
- For-in também enumera índices de strings UTF-16 e propriedades estáticas herdadas de classes, dentro dos limites do snapshot existente.
- Number, String e BigInt, incluindo adapters call/apply/bind e os construtores representáveis, consultam os hooks de conversão. Number admite conversão explícita de BigInt; String conserva unidades UTF-16 e distingue a chamada String(Symbol) da construção. New BigInt continua recusado. AsIntN/asUintN validam bits antes de chamar o hook do valor, sem repetir conversões.
- A revisão também corrigiu conclusões vazias: declarações/statement vazio não encerram prematuramente switch, e finally vazio não substitui return/throw anterior.

## Limites

Objetos nativos especiais gerenciados pela stdlib, incluindo Date, Map, Set, buffers, views e iteradores, e valores Host/Native recusam conversão por esse caminho. Isso evita adotar uma regra comum incorreta para Date ou ignorar métodos sobrescritos de objetos especiais. Não há Proxy.

Outras APIs nativas ainda usam caminhos anteriores de conversão; Math, parseInt, parseFloat e isNaN não foram migrados nesta etapa. O armazenamento de nomes de propriedade continua UTF-8: chaves com surrogates isolados são recusadas explicitamente, sem substituição de caracteres. O limite por chave é 1024 bytes. Super aceita as chaves string convertidas, mas super com Symbol é recusado enquanto seu receiver não estiver representado corretamente. Boxing de Symbol permanece fora do subconjunto.

A conversão padrão de Function depende dos métodos de protótipo já existentes. Arrays cíclicos são limitados por orçamento, sem alegação de comportamento padrão completo de join cíclico.

## Entrega

Após integração e formatação, cargo check --workspace --all-targets --offline --locked passou. Essa checagem compila as fontes, incluindo testes-fonte, mas não os executa e não comprova comportamento funcional.

Foram acrescentados testes-fonte de ordem de efeitos, hints, wrappers, getters, atribuições, chaves/símbolos, UTF-16, herança, erros e limites. A fixture javascript-coercion.html foi preparada. Nenhum teste, fixture ou aplicação foi executado. Não houve build release, atualização do executável distribuído, empacotamento, commit, push ou publicação.
