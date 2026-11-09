# Descritores de propriedades do global Window

A VM passa a aceitar Object.defineProperty e Object.defineProperties no objeto global fornecido pelo host, sem habilitar descritores de outros objetos Host. Uma regressão sintética verifica a definição de Window.Map por um polyfill, com value função e writable/configurable verdadeiros. O teste cobre a operação de definição sem presumir conformidade completa do Map.

## Contrato implementado

- Propriedades string e símbolo ficam no heap da própria VM, com value/get/set, writable, enumerable e configurable reais. Object.getOwnPropertyDescriptor, Object.hasOwn e os métodos correspondentes do protótipo Object observam os metadados sem chamar getters.
- Leituras e escritas por nome global e pelas referências window/self/globalThis mantêm a mesma propriedade. Getters e setters recebem o objeto global como this; exceções, atribuições strict, SameValue e propriedades não configuráveis seguem o caminho comum de descritores.
- Globais intrínsecos substituíveis, como Map, conservam as flags anteriores ao receber uma nova definição ou atribuição. Substituir Window.Map não modifica o construtor nativo anterior. Apagar um global configurável não revela novamente o intrinsic original.
- var/function, inicializadores, padrões e laços usam o mesmo estado da propriedade. Let/const permanecem lexicais, incluindo TDZ. Funções globais declaradas por scripts são não configuráveis; propriedades novas de eval global não estrito são deletáveis. Eval, Function, módulos e scripts inline aninhados compartilham o global sem capturar o escopo local de outro script.
- GC enraíza a tabela de propriedades e seus valores/accessors/símbolos. Rollback restaura metadados e bindings junto com a VM/DOM. Os limites existentes de propriedades, nomes, símbolos, alocação, instruções e chamadas permanecem ativos.

A identidade autorizada vem de Host::global_object, é fixada na execução e não pode ser trocada por outra identidade durante a vida da VM. Nenhum dado da página escolhe esse handle e nenhum novo acesso de rede ou arquivo foi introduzido.

## Limites explícitos

Não é a implementação completa de WindowProxy/WebIDL. Propriedades fornecidas pelo host, como document e location, conservam seus caminhos especializados e recusam descritores neste recorte. O contrato anterior de seus bindings é preservado. Enumeração completa das chaves Window, integridade/protótipo do global e operações Reflect sobre Host continuam recusados, em vez de apresentar uma lista parcial como completa. A definição de uma propriedade não concede autoridade de I/O ou muda políticas do broker.

A fixture fixtures/javascript-window-descriptors.html verifica o polyfill global e um accessor. Validação local Linux executada:

- `cargo test -p web-javascript -p web-bindings --locked --offline`: 504 testes da VM e 186 de bindings passaram, sem falhas ou ignorados. Incluem 24 regressões novas da VM e quatro de bindings para este recorte, além do diagnóstico paralelo.
- `cargo clippy -p web-javascript -p web-bindings --all-targets --locked --offline -- -D warnings` e `cargo fmt --all --check`: aprovados.
- `cargo run -p browser-cli --locked --offline -- render fixtures/javascript-window-descriptors.html --output /tmp/noir-window-descriptors.json`: zero warnings; display list confirmou “Polyfill global e descritores ativos.”
- Revisão independente apontou casos de flags de function/var, exclusão/recriação de intrinsics e execução de módulo antes de scripts clássicos; todos receberam correções e regressões.

A validação focal e a fixture não demonstram Google Search funcional. A nova fonte ainda precisa de validação agregada e QA Windows no commit publicado. As quotas de parsing, arrays e strings não foram ampliadas neste recorte.
