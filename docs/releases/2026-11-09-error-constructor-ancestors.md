# Ancestrais conhecidos dos construtores Error

O recorte permite usar Error, TypeError, RangeError, ReferenceError e SyntaxError como ancestrais internos de objetos e funções ordinárias da VM por Object.setPrototypeOf e Reflect.setPrototypeOf. O caso de compatibilidade é a herança estática de um construtor de página a partir de Error. Isso não altera sua propriedade pública `prototype`, identidade, chamada ou capacidade de construção.

## Contrato implementado

- Error tem Function.prototype como pai interno. TypeError, RangeError, ReferenceError e SyntaxError têm Error como pai. A caminhada continua por Function.prototype, Object.prototype e null, com a mesma contagem limitada de passos.
- Um inventário interno somente de leitura descreve name, length e prototype dos cinco construtores. Name/length são não graváveis, não enumeráveis e configuráveis; prototype é não gravável, não enumerável e não configurável. Os valores continuam sendo os próprios Native, sem converter construtores em objetos ordinários ou criar owners fictícios.
- Get, existência, atribuição string/símbolo, Reflect sobre alvos VM, a caminhada ordinária de instanceof, isPrototypeOf, for-in e super estático atravessam a cadeia real. Accessors herdados de objetos VM recebem o receptor original ou explícito. Atribuição respeita os descritores herdados, mas definição própria no receptor VM conserva seu contrato ordinário.
- Consultas de presença, metadados e enumeração não executam getters nem materializam Error.prototype apenas para testar sua presença ou flags. A leitura efetiva do valor pode materializá-lo pelas rotas de alocação já limitadas da VM. Enumeração de nomes conhecidos consome trabalho e armazenamento de strings.
- Object.getPrototypeOf e, por dispatch específico, Reflect.getPrototypeOf leem o pai desses cinco construtores. O gate genérico de Reflect permanece restrito.
- Bind captura o pai interno antes das leituras observáveis de length/name. Length só é lido quando próprio; remover o length de uma função não passa a usar o length de Error nem um getter herdado. Uma função bound conserva esse pai depois que o alvo muda. Uma arrow que herda de Error continua não construtível, inclusive quando bound.
- Verificação de ciclos inclui todos os ancestrais intrínsecos antes do null terminal e só grava após completar a caminhada. Falhas de profundidade ou instruções continuam Resource, sem escrita parcial. GC e rollback usam as arestas e budgets já existentes.

## Fronteiras preservadas

Os cinco construtores não adquirem mutação de protótipo, propriedades ou símbolos, nem operações de integridade ou inspeção pública de propriedades próprias. Reflect.get/has/set/defineProperty/deleteProperty/ownKeys/getOwnPropertyDescriptor/isExtensible/preventExtensions diretamente nesses Native continuam recusados. A exceção explícita é getPrototypeOf. O inventário de caminhada não é uma autorização de heap_owner ou descriptor_owner.

Host, global Window, nativos arbitrários, AggregateError e exóticos conservam suas fronteiras anteriores. Object.create(Error) permanece não suportado. Não há novo suporte a Symbol.hasInstance customizado. Este recorte não amplia Proxy, newTarget alternativo, construção de classes, I/O ou quotas de parser, propriedades, strings, arrays, chamadas, heap e instruções.

Também permanece fora do recorte atribuir um Native Error à propriedade pública de instâncias, como `F.prototype = Error`. É uma limitação preexistente distinta: a construção ordinária atualmente usa Object.prototype nesse caso, e instanceof com esse prototype nativo é recusado. Não se deve interpretar o suporte a Object.setPrototypeOf(F, Error) como suporte a essa outra operação. Um caminho que passa por Error.prototype não é um caminho pelo pai interno de Error.

A caminhada de super estático não corrige outra limitação preexistente: quando o ancestral tem uma propriedade de dados gravável e o receiver possui um accessor próprio da mesma chave, a atribuição pode chamar o setter do receiver em vez de recusar com TypeError. O caso permanece delimitado, sem alegação de conformidade geral de super.

## Verificação

A suíte específica cobre as cinco cadeias, diferenças entre o slot interno e prototype público, flags herdadas, receivers string/símbolo, ausência de getters em has/metadata/for-in, bind, arrow não construtível, super, rejeições diretas, GC/rollback e limites de caminhada. A fixture sintética `fixtures/javascript-error-ancestors.html` exercita herança de Error e atualização do DOM sem recursos externos.

Validação focal Linux, após a restauração do ambiente, com Rust 1.99.0 e um compilador por vez:

- `cargo test -p web-javascript error_ancestor_tests --locked --offline`: 16 testes novos aprovados, sem falhas ou ignorados.
- `cargo test -p web-javascript --locked --offline`: 805 testes aprovados, sem falhas ou ignorados; doc-tests sem casos e sem falhas.
- `cargo clippy -p web-javascript --all-targets --locked --offline -- -D warnings`: aprovado, sem warnings.
- `cargo fmt --all --check` e `git diff --check`: aprovados.

Integração Linux concluída em 2026-11-10, no mesmo código:

- `cargo test -p web-bindings -p web-renderer -p browser-cli --locked --offline`: 199 testes de bindings, 16 de renderer e 35 do CLI aprovados, sem falhas. Inclui a nova fixture em thread de 1 MiB e uma sessão retida com script inline dinâmico e microtasks.
- `cargo build -p browser-cli --locked --offline`: aprovado.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, fmt e diff-check: aprovados.
- Treze fixtures sintéticas pelo CLI, com pilha de 1 MiB, retornaram o texto esperado sem warnings. A nova fixture Error produziu 14 itens e `PASS: heranca Error, receivers e limites.`.

A execução integral dos testes do workspace e o QA Windows são gates distintos destes resultados focais. Testes sintéticos não comprovam funcionamento geral de sites. Este incremento permanece sem publicação.

A revisão independente confirmou a operação Function → Error e a correção de bind, com oracle externo Node e baseline anterior. Foram aprovados 171 casos negativos principais e 45 controles suplementares admitidos. Dos 124 casos admitidos originais, 119 passaram; os cinco restantes dependiam de this global em um Host de teste vazio, limitação reproduzida na baseline. Os cinco controles com receiver explícito passaram. As sondas de metadados, enumeração, profundidade, trabalho, GC e rollback não encontraram nova expansão de autoridade, escrita parcial ou reposição de budgets. As limitações anteriores de super e Symbol.hasInstance foram mantidas separadas dos resultados do novo recorte.
