# Roadmap: funções numéricas observáveis do item 1

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

parseInt, parseFloat, isNaN e isFinite usam agora o caminho de coerção da VM com acesso ao Host para hooks chamáveis. Number.isNaN/isFinite/isInteger/isSafeInteger são predicados sem coerção; objetos, strings e BigInt retornam false. Number.parseInt e Number.parseFloat são aliases das funções globais. Métodos têm aridades próprias e não são construtores.

- parseInt converte primeiro a entrada em string e depois o radix por ToNumber/ToInt32. Reconhece sinal, radix padrão/hexadecimal, espaços ECMAScript e o primeiro dígito inválido; conserva zero negativo. Decimal usa conversão f64; bases potência de dois acumulam bits de guarda/sticky para arredondar uma vez. Outras bases usam aproximação permitida pela especificação, sem promessa de valor matemático exato para entradas enormes.
- A conversão completa de strings binárias/octais/hexadecimais por Number e operadores também reutiliza o arredondamento único, sem aceitar sufixos. Isso evita perder bits intermediários antes de converter um radix textual por ToInt32.
- parseFloat lê o prefixo decimal em uma passagem, incluindo Infinity, fração e expoente somente quando completo. Prefixos incompletos retornam o último prefixo válido, sem reparsar sucessivamente toda a entrada. Não converte hexadecimal como inteiro.
- A gramática opera em unidades UTF-16: surrogate isolado encerra o prefixo, sem exigir que a string inteira vire UTF-8. ToString rejeita Symbol; BigInt pode ser convertido em texto. Radix e predicados globais rejeitam BigInt/Symbol como números implícitos.
- isNaN/isFinite globais observam Symbol.toPrimitive/valueOf/toString; predicados Number.is* não invocam esses hooks. Argumentos extras não utilizados não são convertidos.

## Limites e entrega

Entradas têm limite conservador de 65.536 bytes para String ou unidades para Utf16, com cobrança prévia de trabalho e alocações. Coerção direta de Host/Native e exóticos da stdlib permanece recusada pelo helper compartilhado; hooks chamáveis em objetos comuns continuam possíveis. Formatação geral de números e outras APIs nativas ainda não estão completas.

Testes-fonte foram acrescentados para gramática, ordem, hooks, aliases, adapters, zeros com sinal, BigInt/Symbol, UTF-16 e limites. A fixture fixtures/javascript-numeric-globals.html foi preparada para execução posterior. Cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram na integração. Nenhum teste, fixture ou navegador foi executado. Sem build release, alteração de dist/Noir, commit, push ou publicação. O item 1 permanece aberto.
