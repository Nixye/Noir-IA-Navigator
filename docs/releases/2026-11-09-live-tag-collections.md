# getElementsByTagName e coleções vivas

Document e Element passam a expor getElementsByTagName para loaders e consultas por tag. A consulta retorna uma coleção viva, sem copiar o resultado para um NodeList estático: length, índices, item e namedItem consultam a árvore atual em ordem de documento. Inserção, remoção, reordenação e innerHTML alteram imediatamente as leituras seguintes. A raiz Element é excluída dos resultados; subárvores destacadas continuam consultáveis. O caractere * seleciona elementos de qualquer tag.

As tags são comparadas sem distinção ASCII no modelo HTML atual. namedItem e propriedades nomeadas observam id/name atuais e usam o primeiro elemento em ordem da árvore; nomes vazios não encontram elementos. length/item/namedItem têm precedência sobre nomes da página. item retorna null quando ausente; leitura de índice/nome ausente retorna undefined. Métodos destacados validam o receptor fornecido com call/apply, expõem aridade 1 e usam conversões observáveis por ToString/ToNumber, incluindo coerção antes da busca viva.

Consultas iguais para a mesma raiz reutilizam a mesma coleção. Há até 64 consultas distintas por documento, nomes/tags até 1024 bytes e traversal sujeito ao orçamento DOM compartilhado. Nenhum getter da página é executado ao consultar presença ou metadados. Esgotamentos de recurso continuam fatais, com rollback e execução desabilitada. A arena DOM mantém os IDs estáveis e a coleção não introduz referências VM não rastreadas.

## Limites

Este é um subconjunto HTML: não implementa namespaces/XML/getElementsByTagNameNS, construtor/protótipo HTMLCollection, descritores/reflexão completa de Host, expandos ou iteradores por símbolo. Array.from usa o caminho array-like já existente: captura length e consulta índices durante o mapeamento. querySelectorAll e os demais NodeLists continuam snapshots.

Template/noscript seguem a fronteira inerte existente do motor: os elementos são visíveis, mas seus descendentes não entram nessas consultas. Erros de receptor/argumento seguem o contrato Host atual e não prometem todas as classes de exceção WebIDL. Não se afirma conformidade DOM/WPT nem compatibilidade de sites reais a partir destes testes.

## Validação

Testes dirigidos e fixture em fixtures/javascript-live-collections.html cobrem liveness, raízes destacadas, ordem, nomes, coerção com mutação/throw, índices unsigned, métodos destacados, receivers incompatíveis, GC, fragmentos, limites, orçamento cumulativo e o padrão de loader que obtém o primeiro script e insere um novo antes dele. Validação local executada:

- `cargo test -p web-bindings --locked --offline`: 177 testes passaram, nenhum falhou ou foi ignorado, incluindo 17 regressões novas de coleções.
- `cargo fmt --all --check` e `cargo clippy -p web-bindings --all-targets --locked --offline -- -D warnings`: passaram.
- `cargo run -p browser-cli --locked --offline -- render fixtures/javascript-live-collections.html --output /tmp/noir-live-collections-final.json`: terminou com zero warnings. O display list contém “Coleção viva e padrão de loader ativos.”
- Revisão independente dos limites, handles e coerções concluída; testes adicionais cobrem coerção reentrante, validação do receptor antes da coerção, rollback, handles inválidos e nomes numéricos.

Esses resultados são locais e focais. Não substituem a validação agregada do workspace, CI nas plataformas alvo ou teste de sites reais.
