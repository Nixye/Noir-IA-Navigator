# Propriedades do construtor Number

O construtor Number conserva sua identidade chamável e passa a usar armazenamento de propriedades no heap da VM, como Symbol. Essa escolha é explícita: outros construtores nativos, métodos nativos e objetos Host não recebem permissão genérica para descritores.

## Contrato implementado

- As 17 propriedades iniciais incluem name, length, prototype, seis métodos numéricos já suportados e oito constantes. MAX_SAFE_INTEGER e MIN_SAFE_INTEGER usam ±9007199254740991; MIN_VALUE é o menor subnormal positivo, 5e-324. As constantes e prototype não são graváveis nem configuráveis. Name/length são configuráveis; métodos são graváveis e configuráveis; todas essas propriedades iniciais são não enumeráveis.
- Number.parseInt/parseFloat conservam a identidade dos aliases globais. Chamada, new, call, bind, o protótipo numérico e instanceof usam as mesmas identidades anteriores.
- Definição, leitura, atribuição, exclusão, has/in, enumeração e Object/Reflect observam os mesmos metadados. Propriedades string e símbolo aceitam dados e accessors; getters/setters recebem Number ou o receiver explícito. Inspeção de metadados não invoca getters. Exclusão de um método não o recria por um fallback nativo.
- Seal, freeze e preventExtensions usam as invariantes comuns da VM. SameValue distingue zeros com sinal e admite NaN. Propriedades herdadas, cópia e spread mantêm os receivers e a enumeração do subconjunto já existente.
- A tabela é local à VM e enraizada no GC; valores, símbolos e closures de accessors permanecem vivos enquanto referenciados. Rollback restaura tabela e metadados, inclusive a inicialização preguiçosa, preservando trabalho consumido.
- O armazenamento conserva os tetos de 128 propriedades string, 128 propriedades símbolo e 1024 bytes por chave, além dos limites normais de heap, strings, instruções e chamadas. Uma inserção recusada em objeto comum não reserva mais uma posição na ordem de propriedades.

A fixture `fixtures/javascript-number-descriptors.html` é sintética e não carrega recursos externos. Ela verifica constantes nativas, instalação condicional de propriedades, flags, accessor e atualização real do DOM.

## Limites preservados

Não há implementação genérica de objetos nativos. Alterar o protótipo interno de Number extensível continua recusado; pedir o mesmo protótipo funciona, e outro protótipo em Number não extensível retorna false via Reflect. Coerção de um valor Native por operadores permanece recusada, inclusive se ele possui Symbol.toPrimitive. Essa mudança não acrescenta APIs de rede/arquivo, não altera sandbox e não aumenta quotas de parsing, arrays ou strings.

Os testes sintéticos e a fixture não comprovam compatibilidade completa com sites externos. A validação integrada e a QA de navegador são etapas separadas.

## Validação focal Linux

- `cargo test -p web-javascript -p web-bindings --locked --offline`: 519 testes da VM e 188 de bindings passaram, sem falhas ou ignorados. O recorte acrescenta 15 testes de VM e dois de bindings.
- `cargo clippy -p web-javascript -p web-bindings --all-targets --locked --offline -- -D warnings`, `cargo fmt --all --check` e `git diff --check`: aprovados.
- `cargo run -p browser-cli --locked --offline -- render fixtures/javascript-number-descriptors.html --output /tmp/noir-number-descriptors.json`: fixture executada com zero warnings; a display list contém “Constantes e descritores Number ativos.”
- Revisão independente do código sem bloqueadores. Testes cobrem os limites exatos de propriedades e símbolos, chaves extensas, orçamentos menores de heap/string, getters sem execução durante inspeção, constantes imutáveis, rollback, GC e ordenação após inserção recusada.

## Validação integrada

Concluída em 2026-11-09: 1001 testes passaram, zero falharam e um teste de inferência real ficou ignorado por exigir modelo/runtime verificados. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` e `git diff --check` passaram. Os 973 arquivos do snapshot permaneceram iguais durante as verificações. Após a validação, apenas este registro documental foi acrescentado; código, testes e fixture não mudaram.
