# Mutação real de protótipos de objetos e funções

Object.setPrototypeOf passa a alterar o protótipo interno de objetos ordinários e funções da VM. Reflect.setPrototypeOf compartilha a mesma mutação. O suporte inclui cadeias Function → Object e Function → Function, sem alterar a propriedade pública function.prototype ou a capacidade de chamar a função.

## Contrato implementado

- Object retorna o próprio alvo no sucesso e lança TypeError quando a mudança é inválida; Reflect conserva seu resultado booleano. Primitivas não nullish são retornadas intactas por Object, depois de validar o novo protótipo. Null/undefined são recusados sem boxing.
- Identidade do protótipo é verificada antes das restrições de extensibilidade. Ciclos diretos/mistos, mudança de Object.prototype e mudanças em objetos não extensíveis são recusados antes de qualquer escrita.
- Funções guardam um slot interno separado de suas propriedades próprias. Get/has, atribuição, símbolos, Object/Reflect, instanceof, isPrototypeOf e for-in observam a cadeia real. Accessors mantêm o receiver original ou o receiver explícito de Reflect.
- A leitura percorre uma única cadeia iterativa limitada a 64 passos, inclusive ao alternar entre objetos e funções. Consultas, escritas e verificação de ciclos continuam consumindo trabalho. Esgotar a caminhada de escrita por símbolo gera Resource antes de gravar qualquer propriedade.
- Bind captura o protótipo da função-alvo antes dos getters observáveis de length/name. Alterações posteriores no alvo não mudam esse protótipo capturado. Super estático acompanha a cadeia de uma função-base ordinária.
- O GC rastreia o slot interno como uma aresta do objeto função, sem transformá-lo em raiz global. Clone/rollback incluem o slot; funções ou protótipos sem referências podem ser coletados.

## Limites preservados

A mutação de arrays, construtores class e construtores nativos continua recusada neste recorte; pedidos de manter uma identidade já representável podem ser aceitos. Não há alteração de iteradores de Array ou do comportamento de super() em construtores class. Native/Host genéricos, RegExp e objetos especiais da stdlib mantêm suas fronteiras. Não há Proxy nem suporte geral a exóticos. Esta mudança não aumenta quotas de parser, strings, propriedades, arrays, heap ou instruções e não adiciona I/O ao motor.

A fixture sintética `fixtures/javascript-function-prototypes.html` verifica as duas formas de herança entre funções, receiver de accessor, bind, ciclo e atualização real do DOM, sem recursos externos. Testes sintéticos não comprovam funcionamento geral de sites.

## Validação focal Linux

- `cargo test -p web-javascript -p web-bindings --locked --offline`: 575 testes da VM e 192 de bindings passaram, sem falhas ou ignorados, na árvore combinada com o pacote de armazenamento. Este recorte acrescenta 19 testes VM e dois de bindings.
- `cargo clippy -p web-javascript -p web-bindings --all-targets --locked --offline -- -D warnings`, `cargo fmt --all --check` e `git diff --check`: aprovados.
- `cargo run -p browser-cli --locked --offline -- render fixtures/javascript-function-prototypes.html --output /tmp/noir-function-prototypes.json`: zero warnings; display list confirmou “Protótipos e receptores de funções ativos.”
- Revisão independente do código sem bloqueadores. A regressão adversarial constrói uma cadeia mista de 80 nós e verifica Resource em leitura, existência, escrita string/símbolo, Reflect, instanceof, isPrototypeOf e for-in, sem propriedades criadas pela operação truncada. GC também cobre bound function cujo protótipo capturado perdeu suas outras referências.

O incremento permanece sem publicação e aguarda validação integrada e QA do pacote maior.
