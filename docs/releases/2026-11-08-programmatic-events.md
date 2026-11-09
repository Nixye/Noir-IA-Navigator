# Item 5: eventos programáticos limitados

Estado: implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–5 ABERTOS/parciais; item 19 adiado. Não foram executados testes, fixtures, aplicativo ou Clippy nesta etapa. Não há reconstrução de dist/Noir, commit, publicação ou nova distribuição.

## Incremento

- Event/CustomEvent host, construídos com new, com bubbles/cancelable false por padrão e detail null ou valor original rastreado pelo GC.
- DispatchEvent síncrono em Window, Document e nós, inclusive destacados; caminho real, captura/alvo/bubble, this/target e opções once/passive existentes.
- Retorno false somente para evento cancelável cancelado, sem ações padrão nativas. O mesmo evento pode ser reutilizado após a propagação; reentrada do próprio evento é recusada.
- Ponte VM de construção host opt-in, com name/length/typeof, bound constructors e Reflect.construct limitados. Prototype, instanceof e class extends host continuam recusados.
- Erros ordinários diagnosticados com continuação; Resource fatal até a fronteira transacional exterior. Dispatch iniciado por JavaScript não intercala checkpoints extras entre listeners.

O [contrato de eventos](../specifications/events-subset.md) detalha limites, identidade e recusas. Não implementa EventTarget independente, eventos especializados, caminhos Shadow DOM, AbortSignal, ações padrão nativas ou entrada/edição completas. O item 5 não está concluído.

## Limites e fronteiras

Tipos até 64 bytes UTF-8; 256 eventos retidos por sessão sem reciclagem individual, incluindo eventos iniciais/nativos; 128 listeners simultâneos. Dispatch aninhado limita-se a 32; o caminho usa MAX_DEPTH do DOM. Heap, instruções e trabalho DOM continuam limitados e compartilhados. Type usa ToString observável, sem Symbol/surrogates isolados; init lê bubbles/cancelable/composed/detail e recusa composed truthy. Tipo vazio não pode ser disparado. Não há novos canais IPC, rede, arquivos ou permissões. O renderer continua recebendo somente recursos autorizados pelo broker.

## Fonte e evidências

A [fixture programmatic-events.html](../../fixtures/programmatic-events.html) oferece botão que emite CustomEvent e imprime a ordem esperada, cancelamento/passive/once, identidade de detail, recusa de reentrada e reuso em nó destacado. A fixture e regressões em código-fonte não foram executadas e não comprovam comportamento funcional.

A revisão textual consolidada foi concluída sem bloqueadores após o guard de parada no início de cada grupo e as regressões-fonte de parada prévia, reuso e capture no alvo. StopPropagation conserva os listeners do grupo atual e impede grupos posteriores, inclusive não-capture no alvo após capture; stopImmediatePropagation interrompe também os restantes do grupo atual. As regressões não foram executadas. Resultados anteriores dos itens 1–4 não validam este incremento. Nenhuma conformidade DOM/WPT, compatibilidade geral de sites ou segurança independente foi demonstrada.

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O processo de check terminou com exit 0 em 2,55 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído. Revisão textual consolidada concluída e checagem estática aprovada. Não houve execução de testes, fixtures, aplicativo ou Clippy. Compilação estática e regressões em código-fonte não comprovam comportamento funcional; item 5 permanece ABERTO/parcial.
