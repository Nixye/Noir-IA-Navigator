# Item 3: documentos HTML normalizados e fragmentos contextuais

Estado: primeiro recorte em implementação parcial. Revisão textual consolidada concluída e checagem estática aprovada. Não houve testes, execução da fixture, navegação, inspeção gráfica, build ou reconstrução de `dist/Noir` nesta etapa. Os itens 1, 2 e 3 permanecem ABERTOS/parciais; o item 19 continua adiado conforme solicitado.

## Contrato do recorte

- O parser de documento constrói um único esqueleto `html/head/body`, inclusive com entrada vazia ou abreviada. Metadados anteriores ao corpo são colocados no head. A recuperação de wrappers repetidos é limitada, não uma implementação integral da construção de árvore WHATWG.
- A seleção básica do modo de documento pelo doctype continua separada da estrutura. Reconhecer quirks/limited quirks não implementa o layout legado completo.
- O parser de fragmentos é separado: entrega filhos sem wrappers sintéticos e não substitui o modo do documento receptor. Contextos ordinários usam o subconjunto de árvore; `script/style/noscript` recebem texto literal e `title/textarea` recebem RCDATA, com entidades do subconjunto decodificadas. Nesse contexto, aparentes tags de fechamento continuam texto do fragmento.
- Contextos de tabelas, select e conteúdo estrangeiro são recusados explicitamente. A lista e a superfície JavaScript constam no [subconjunto HTML/DOM](../specifications/html-dom-subset.md).
- `innerHTML` possui **setter**, não getter/serializador. O setter prepara a árvore temporária e valida quotas, profundidade e destino antes de substituir filhos; erros não deixam uma substituição parcial. IDs são remapeados sem reciclar identidades. Scripts inseridos não são executados.
- `document.documentElement`, `document.head` e `document.body` refletem a estrutura conectada real. Não há fallback de body para Document. A remoção de head/body faz o getter indicar ausência, sem recriação silenciosa.
- `document.title` vem do primeiro título ativo em ordem da árvore, com normalização de espaços HTML, e retorna vazio na ausência. Não reutiliza o fallback visual da interface. A escrita atualiza o título existente ou cria um título no head adequado; sem título e sem head, não faz nada.

## Inatividade e consumidores

A recuperação de end tags e de option/optgroup respeita a fronteira de template: não pode fechar ancestrais externos e fazer conteúdo inativo escapar. `noscript`, com scripting habilitado, é texto literal/inativo. Script, link, style e img dentro de template não são promovidos para head/body. End tags incompletas em raw text/RCDATA conservam todo o sufixo no elemento original, sem reinterpretar esse conteúdo como markup ativo. A correção foi identificada por leitura de código; não constitui relato de ataque executado ou bypass de sandbox.

Wrappers reais alteram a aplicação de `:root`, herança e margem padrão de body (8 px). `:root` deve identificar o elemento real de documento, não qualquer html aninhado ou filho da raiz. Capturas anteriores podem mudar; não se promete equivalência de pixels nem se removem margens corretas para reproduzir imagens antigas. A revisão dos consumidores CSS/layout/broker integra este recorte, sem ampliar política de recursos. Essas mudanças estruturais e visuais não receberam QA gráfico.

## Limites e fronteiras preservadas

HTML continua limitado a 1 MiB, DOM a 20.000 nós e profundidade 128, incluindo o esqueleto criado pelo parser. O setter innerHTML mantém 64 KiB e os orçamentos DOM cumulativos existentes. Quotas podem recusar entradas antes de qualquer limite individual; a árvore temporária não concede capacidade adicional. Falhas de quota do parser (entrada, nós, profundidade e transições de modo de inserção) são convertidas em Resource, preservando a política fatal e o rollback da operação existente. A importação cobra nós e strings contra os limites cumulativos de 2.048 mutações e 256 KiB; pode falhar antes do teto de 20.000 nós da arena. Contextos não suportados continuam erros runtime ordinários.

Não há novo I/O, rede, IPC, dependência de browser engine ou política de sandbox. O broker continua sendo a única origem dos bytes autorizados.

## Pendências explícitas

Construção WHATWG completa, modos de inserção de tabelas/foster parenting/adoption agency, nós Comment e DocumentType, namespaces SVG/MathML, DocumentFragment público em JavaScript, template.content, import/adopt entre documentos, coleções vivas, Web Components, observers e layout quirks completo continuam ausentes ou incompletos. Elementos com esses nomes não comprovam suporte semântico.

## Fonte de demonstração e evidência

[html-normalized-documents.html](../../fixtures/html-normalized-documents.html) é uma fixture-fonte abreviada para estrutura, metadados, getters, setter innerHTML sem wrappers, título, contextos e inatividade de template. Não foi executada e não é resultado de validação. Documentos vazios e matrizes completas de entradas malformadas não são demonstrados por essa única página.

Revisão textual consolidada concluída, incluindo a classificação de quotas do parser como Resource e a preservação de end tags incompletas em raw text/RCDATA. Foram concluídos `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked`. A checagem terminou com código 0 (1,92 s), apenas com o aviso preexistente de `Stmt::Function`.

A fonte `incomplete_raw_end_tags_keep_the_entire_suffix_in_the_original_element`, em `crates/web-html/src/lib.rs`, acompanha a correção de raw text. Compilar fontes de casos não significa executá-los nem observar seus resultados. Não foram executados testes, fixtures, aplicativo, Clippy, Test262, WPT ou validação visual. Sem build release, reconstrução de dist, commit ou publicação. Não se exige nem se promete executar testes dispensados pela usuária; aprovação estática não comprova comportamento funcional ou compatibilidade de sites.

O item 4 (ciclo de vida/defer) está somente em planejamento, não implementado por esta entrega. Os itens 1, 2 e 3 permanecem ABERTOS/parciais.
