# Seletores DOM/CSS compartilhados: segundo recorte do item 3

Data: 2026-11-08. Estado: implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada. Sem validação funcional.

## Contrato

querySelector/querySelectorAll, matches e closest passam a reutilizar o parser e o matcher CSS próprios por `web_css::selector_query`, substituindo o parser simples independente dos bindings. A lista opaca SelectorList é validada por inteiro antes de procurar candidatos: uma alternativa inválida ou fora do subconjunto rejeita toda a consulta, sem recuperação de stylesheet ou resultado parcial.

O subconjunto inclui tipos, IDs, classes, universal, compostos, listas, descendente, filho (>), irmãos adjacente (+) e subsequente (~), atributos e seus operadores/flags i/s existentes, escapes CSS existentes, :is/:where/:not, :root, :empty, first/last/only-child e of-type, as variantes nth-child/nth-of-type, :checked/:disabled/:enabled, :link/:any-link e :lang. A semântica continua a do matcher atual, inclusive estados baseados nos atributos e linguagem limitada; isso não implementa todos os estados de formulário ou CSS Selectors. :scope, :has, namespaces, pseudoelementos e Shadow DOM continuam fora do subconjunto. Listas lógicas também não são forgiving.

## Consulta e identidade

- A enumeração segue a ordem da árvore atual, não a ordem das alternativas nem os IDs da arena. Um elemento aparece uma vez mesmo quando corresponde a vários seletores.
- Document consulta seus descendentes; Element consulta descendentes sem incluir o receptor. Ancestrais/irmãos fora da raiz consultada ainda podem participar do matching. Isso não equivale a suporte a :scope.
- querySelector interrompe a travessia incremental no primeiro resultado; querySelectorAll guarda snapshot estático dos nós reais. Mutações posteriores não alteram os membros desse snapshot. Não há coleção viva nem novo iterador DOM.
- matches testa o receptor. closest inclui o próprio elemento e sobe até o primeiro ancestral correspondente, inclusive em subárvores destacadas.
- A enumeração preserva as fronteiras de template/noscript: os próprios elementos podem ser candidatos, mas seus filhos não são percorridos. Consultar diretamente esses receptores não expõe os filhos. Essa fronteira de enumeração não proíbe matches/closest sobre um nó inativo já referenciado; essas operações consultam o próprio nó/ancestrais. Não acrescenta template.content ou DocumentFragment.

## Limites, erros e arquitetura

A fachada aceita até 16 KiB de texto por lista, 64 alternativas superiores e 1024 bytes por alternativa após trim de whitespace CSS. Exceder esses limites produz Resource. Limites anteriores do parser, incluindo profundidade 8, 32 compostos por seletor e 16 alternativas internas lógicas/lang, continuam delimitando o subconjunto aceito; a recusa é SyntaxOrUnsupported. Não há promessa de CSSOM ou DOMException completas.

Os bindings convertem SyntaxOrUnsupported em erro runtime ordinário `Invalid DOM selector syntax or unsupported selector feature`. Resource usa a política existente de erro fatal, rollback e orçamento preservado. Nas chamadas JavaScript, o contexto é único por operação e recebe o saldo do orçamento acumulado read_work do Host, de 2 milhões de unidades de leitura DOM. O helper Rust público DocumentSession::query_selector usa orçamento local por operação, não o acumulado da sessão JS. Cobra bytes da entrada mais uma unidade antes do parsing, travessia e matching, inclusive quando há erro. O índice de irmãos é construído sob demanda uma vez, com cobrança da arena/arestas, não uma vez por candidato. A contabilidade compartilhada também passa a cobrar todo o atributo class antes da tokenização, filhos da raiz em :root e bytes de tags em nth/only-of-type; isso pode antecipar Resource sem alterar a semântica de correspondência. Parar no primeiro resultado não elimina o custo desse índice global nem constitui medição de desempenho.

A dependência direcionada web-bindings → web-css reutiliza o motor próprio e não introduz rede, arquivos, IPC ou nova fronteira de confiança. Não há novo ADR de isolamento: as fronteiras existentes permanecem. Limites de nós/profundidade, listas e sessão continuam aplicáveis.

## Fontes preparadas, não executadas

- `crates/web-css/src/selector_query.rs`: fachada pública limitada e sete fontes de regressão do parser/matcher, não executadas.
- `crates/web-bindings/src/lib.rs` e `selector_query_tests.rs`: integração e oito fontes de regressão das consultas DOM, não executadas.
- [Fixture-fonte HTML](../../fixtures/shared-dom-selectors.html): consultas compostas, ordem/deduplicação, snapshot, ancestrais externos, escapes, closest destacado e recusa integral.

Revisão textual consolidada liberada sem bloqueadores. Checagem estática central das fontes finais aprovada: `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked`; sessão 1083, saída 0, duração informada de 2,01 s. Permanece apenas o warning preexistente de `Stmt::Function` em `parser.rs:179`. Esses comandos formatam/verificam fontes e checam alvos, mas não executam as sete fontes de regressão CSS ou as oito DOM nem validam seu comportamento. Nenhum teste, JavaScript, fixture, aplicativo, navegador, rede, Clippy ou build de distribuição foi executado neste recorte. Fontes de regressão e saídas esperadas não são evidência funcional. O histórico da primeira passagem, de Array.from e de i64/BigInt permanece separado e inalterado. Sem commit, push, publicação ou reconstrução de dist/Noir.

Itens 1–18 continuam ABERTOS/parciais; item 19 com EXECUÇÃO ADIADA. Este incremento não conclui o item 3.

- [Subconjunto HTML/DOM](../specifications/html-dom-subset.md)
- [Contrato JavaScript](../specifications/javascript-subset.md)
- [Primeiro recorte do item 3](2026-11-08-normalized-html-documents.md)
