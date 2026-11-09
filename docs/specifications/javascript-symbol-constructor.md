# Propriedades do construtor Symbol

Este incremento permite propriedades reais no objeto da função intrínseca
`Symbol`. Não implementa todos os objetos nativos, todos os well-known Symbols
nem o protocolo de descarte de recursos. `Symbol.dispose` começa ausente e só
passa a existir se o próprio script o definir.

## Identidade e armazenamento

`Symbol` continua sendo a mesma função nativa chamável, não construtível. O
armazenamento de suas propriedades usa um objeto interno do heap da VM, sem
expor esse objeto por `Object(Symbol)`, `constructor`, descritores ou consultas
de protótipo. `Object.getPrototypeOf(Symbol)` e `Reflect.getPrototypeOf(Symbol)`
retornam a identidade chamável `Function.prototype`.

As propriedades intrínsecas existentes são materializadas uma vez por runtime:

- `length` (0) e `name` (`Symbol`): não graváveis, não enumeráveis, configuráveis.
- `prototype`: não gravável, não enumerável, não configurável; conserva a
  identidade anterior de `Symbol.prototype`.
- `for` e `keyFor`: graváveis, não enumeráveis, configuráveis. Seus valores
  continuam funções nativas e o comprimento declarado de cada método é 1.
- `asyncIterator`, `hasInstance`, `isConcatSpreadable`, `iterator`, `match`,
  `replace`, `search`, `species`, `split`, `toPrimitive`, `toStringTag` e
  `unscopables`: não graváveis, não enumeráveis, não configuráveis, com as
  identidades de símbolos já oferecidas pela VM.

A lista acima é o subconjunto anterior, não uma lista de conformidade completa.
Não se acrescenta `matchAll`, `dispose`, `asyncDispose` ou suporte aos protocolos
que apenas mencionar esses nomes poderia sugerir.

## Operações

Leitura, atribuição, existência, exclusão, descritores de dados/acessores e
enumeração usam o mesmo armazenamento. Chaves string e Symbol são distintas.
São aceitos `Object.defineProperty/defineProperties/getOwnPropertyDescriptor`,
`getOwnPropertyDescriptors/getOwnPropertyNames/getOwnPropertySymbols`,
`keys/values/entries/hasOwn`, os métodos de inspeção de `Object.prototype`,
`Object.assign`, spread e os níveis de integridade de Object.

`Reflect.defineProperty`, `set` e `deleteProperty` retornam false para rejeições
reais por atributos ou não extensibilidade. Falhas de conversão, getters,
setters e cotas continuam erros, não são transformadas em false. A variante
Object de definição inválida lança TypeError. Escritas/exclusões diretas
respeitam os modos estrito e não estrito do subconjunto existente.

Consultas de existência e descritores não executam getters. Leitura/atribuição
de acessores preservam o receptor real, inclusive `Symbol`, objetos que o usam
como protótipo e um receptor distinto passado a Reflect. O percurso encontra
as propriedades já expostas de `Function.prototype` (`length`, `name`,
`constructor`, `call`, `apply`, `bind`) e depois `Object.prototype`. A tabela
desse protótipo é somente leitura: seus métodos são não enumeráveis,
graváveis/configuráveis, e name/length são não graváveis/configuráveis.
Seu name é a string vazia e length é 0, tanto na leitura como no descritor.

Não se acrescenta uma implementação completa de `Function.prototype`, como
seu `toString` específico ou `@@hasInstance`. Definições e exclusões diretamente
nesse objeto continuam recusadas, assim como descritores dos demais nativos e
de objetos Host/DOM. A leitura limitada não concede autoridade ao host.

`Reflect.setPrototypeOf(Symbol, Function.prototype)` é um no-op verdadeiro.
Alterar o protótipo de Symbol extensível continua uma operação não implementada
e lança TypeError. Depois de impedir extensões, uma tentativa de trocar esse
protótipo retorna false. `Object.setPrototypeOf` mantém sua limitação anterior.

`Object.getOwnPropertyDescriptors` também conserva as chaves Symbol dos alvos
aceitos, sem executar seus acessores. Seu resultado continua limitado a 128
descritores somando nomes e símbolos.

## Limites e memória

Nenhum orçamento foi ampliado. Symbol tem até 128 propriedades textuais,
incluindo suas 17 intrínsecas existentes, e até 128 propriedades com chave
Symbol. Nomes continuam limitados a 1024 bytes. Enumeração, strings,
metadados, objetos, símbolos, profundidade de protótipos e callbacks usam os
orçamentos da VM. Falha por cota não declara uma definição bem-sucedida.

O armazenamento interno é uma raiz explícita por runtime. Seus valores,
chaves, getters e setters passam pelos mecanismos já existentes de GC; remover
propriedades libera as referências. Clones transacionais mantêm esse estado e
runtimes distintos não compartilham alterações de propriedades.

## Reprodução e limites da evidência

`crates/web-javascript/src/fixtures/symbol-constructor-polyfill.html` é uma
fixture sintética, sem rede, que verifica ausência inicial de dispose, definição
real pela página, identidade da função e flags. O teste unitário usa o mesmo
script da fixture. Não contém um script remoto capturado.

A inspeção estática do snapshot HTTPS público do Google, ciclo 16, encontrou
uma instalação de polyfill para `Symbol.dispose` anterior às demais operações
que exigiriam descritores de nativos. A regressão sintética reproduziu o erro
anterior `Host/native exotic descriptors are not implemented` e passou após a
correção. Isso por si só não prova execução completa do script remoto, da
página ou da busca. A comparação do site deve usar somente replay HTTPS pelo
AppContainer normal e registrar separadamente quaisquer erros seguintes.

Os testes específicos exercitam atributos intrínsecos, identidade, receivers,
acessores herdados, ordem e exclusão, chaves Symbol, Reflect false versus throw,
conversão e erros de callbacks, seal/freeze/preventExtensions, limites exatos,
retenção/liberação pelo GC e ausência de estado entre runtimes.
