# Subconjunto HTML, documentos e DOM

O [primeiro recorte do item 3](../releases/2026-11-08-normalized-html-documents.md) está em implementação parcial; revisão textual consolidada concluída e checagem estática aprovada. A fonte não foi validada funcionalmente. O item 3 permanece ABERTO.

## Documento e fragmento

O parser próprio de documento normaliza entradas vazias/abreviadas em html, head e body únicos. Metadados antes do corpo vão para head; html/body repetidos incorporam somente atributos ainda ausentes; head repetido é ignorado. Metadados posteriores ao início do corpo permanecem na posição de inserção, sem reorganização retroativa. Modos básicos de doctype são metadados, não promessa de layout quirks completo. A árvore não expõe nós Comment ou DocumentType.

Fragmentos têm entrada própria, sem esqueleto de documento e sem alterar o modo do receptor. Nos contextos script/style/noscript, toda a entrada é texto literal. Nos contextos title/textarea, toda a entrada é RCDATA com a decodificação de entidades disponível, inclusive aparentes end tags. Contextos ordinários usam recuperação limitada.

Contextos recusados: table, tbody, thead, tfoot, tr, td, th, caption, colgroup, select, optgroup, option, svg, math e foreignobject. O parser retorna `Unsupported HTML fragment context`; no binding é erro runtime, não DOMException padronizada. A recusa não habilita um parser alternativo nem transforma esses contextos em div. Construção de tabelas, select e namespaces estrangeiros completa permanece pendente.

## Bindings

- documentElement é o elemento conectado diretamente ao Document; head/body pertencem à estrutura real, sem busca arbitrária em subárvores inativas e sem body substituído pelo Document.
- Getters refletem remoções posteriores. O parser normaliza a entrada; getters não reparam silenciosamente a árvore mutada.
- document.title usa texto DOM real do primeiro título ativo em ordem da árvore, normalizando espaços HTML. Ausência retorna string vazia. O fallback visual é responsabilidade separada da interface. O setter atualiza o título encontrado ou cria no head adequado; sem ambos, é no-op.
- innerHTML oferece somente escrita. O setter usa o parser contextual, preflight de importação, remapeamento de IDs sem reciclagem e troca de filhos somente após validação. Não há getter/serializador nem roundtrip prometido. Scripts inseridos permanecem não executados. O modo do documento receptor é preservado.
- APIs de criação, movimento e clonagem do subconjunto anterior continuam limitadas a um documento. DocumentFragment JavaScript, template.content, importNode/adoptNode completos, coleções vivas, Shadow DOM, custom elements e observers continuam pendentes.

## Inatividade, limites e aparência

Template conserva sua fronteira durante recuperação de end tags e option/optgroup; não promove script/link/style/img para regiões ativas. Noscript é inativo/raw text com scripting habilitado. End tags incompletas em raw text/RCDATA mantêm o sufixo no elemento original. Isso não implementa template.content ou os algoritmos completos de template.

O parser mantém HTML de até 1 MiB, 20.000 nós e profundidade 128, contando wrappers. innerHTML mantém 64 KiB e os orçamentos DOM já existentes; falhas de preparação preservam os filhos anteriores. Quotas do parser de entrada, nós, profundidade e transições de modo são Resource e seguem a política transacional existente. A importação também cobra cada nó e strings contra 2.048 mutações e 256 KiB cumulativos, podendo falhar antes do limite da arena.

A existência real de html/body faz :root, herança e margem padrão de 8 px do body incidirem sobre documentos abreviados. Sem equivalência visual garantida com capturas antigas. Não há mudança de I/O, IPC, rede ou sandbox.

A [fixture-fonte](../../fixtures/html-normalized-documents.html) acompanha o contrato sem ter sido executada.
## Segundo recorte: consultas por seletores CSS

querySelector/querySelectorAll e matches/closest usam a lista estrita e o matcher de web-css, não um parser DOM paralelo. Veja o [contrato e limites](../releases/2026-11-08-shared-dom-selectors.md). Uma alternativa inválida rejeita toda a lista antes da busca. A enumeração usa ordem da árvore e não duplica candidatos que casem com várias alternativas; querySelector termina no primeiro resultado, sem percorrer todos os descendentes antecipadamente.

Consultas em Element excluem o receptor dos candidatos, mas ancestrais/irmãos fora dele podem participar do matching. closest inclui o próprio elemento e funciona destacado. querySelectorAll mantém snapshot de membros reais, acessíveis por length/índices e Array.from; não vira coleção viva ou iterável DOM. Os filhos de template/noscript continuam fora da enumeração, inclusive na consulta direta desses receptores. Essa fronteira de enumeração não impede matches/closest sobre nós inativos já referenciados. Não há template.content, Shadow DOM ou :scope.

Nas chamadas JavaScript, o contexto único por operação cobra parsing, índice sob demanda, matching e travessia contra o saldo acumulado de leitura DOM do Host, inclusive nas falhas. O helper Rust DocumentSession::query_selector usa saldo local por operação. Erro runtime de sintaxe/subconjunto não é DOMException completa; Resource mantém a política transacional. A dependência web-bindings → web-css não acrescenta I/O ou IPC. [Fixture-fonte](../../fixtures/shared-dom-selectors.html) preparada, não executada. Implementação parcial; revisão textual consolidada concluída sem bloqueadores e checagem estática central aprovada, sem validação funcional. Item 3 permanece ABERTO; item 19 com EXECUÇÃO ADIADA.
