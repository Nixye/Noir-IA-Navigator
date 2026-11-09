# Item 11: sessionStorage limitado em memória

Estado: implementação parcial; revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada. Sem validação funcional. Itens 1–11 ABERTOS/parciais; item 19 adiado. Aprovações dos itens anteriores permanecem históricas e não validam este recorte.

## Entrega de código-fonte

O recorte integra sessionStorage real em memória por aba e origem HTTP(S), sem simular sucesso nem persistir dados no perfil. Global/window compartilham identidade host com length, key, getItem, setItem, removeItem e clear. Ordem de inserção; sobrescrita mantém posição, remoção/reinserção vai ao fim, ausência retorna null. Conversões são observáveis, com chave antes de valor.

A shell retém TabSessionStorage; o supervisor escolhe origem pela URL final do broker. Só o bucket atual chega ao renderer antes dos scripts. IPC v5 transporta seed/snapshot limitado e validado nos dois extremos; nenhuma autoridade de rede, arquivo, credenciais ou IA é acrescentada. Veja o [ADR 0017](../adr/0017-bounded-session-storage.md) e o [subconjunto](../specifications/session-storage-subset.md).

## Limites e vida útil

- 128 pares/origem, chave 1 KiB, valor 16 KiB, soma 64 KiB, em bytes UTF-8; surrogate isolado recusado. Trabalho host cumulativo limitado a 2.000.000 unidades, além dos budgets da VM.
- 16 origens retidas/aba. Origem nova vazia não ocupa slot; bucket existente vazio permanece. Sem eviction. Origem excedente lê vazio e setItem falha de forma capturável.
- Refresh, A→B→A, histórico e HOME/SETTINGS conservam estado da aba. Abas/workspaces não compartilham buckets. Close elimina; restart não restaura.
- file/noir/data/origem opaca/ausente são recusados, sem bucket null.
- Quota de dados é erro ordinário atômico; Resource de trabalho é fatal e participa do rollback de DOM/VM, sem devolver orçamento VM consumido; a falha fatal desabilita execução futura.
- Snapshot só é confirmado após validar a resposta inteira. Navegação inicial superada descarta escritas; operação de sessão já comprometida pode guardar checkpoint antigo confirmado antes da próxima navegação, sem publicar página obsoleta. Crash/timeout/frame inválido conserva último checkpoint confirmado, não writes pendentes.

O teto de frame é 48 MiB; quotas semânticas menores são aplicadas após desserialização JSON, que já aloca strings/vetores. Não se promete alocação total de apenas 64 KiB. O navegador não inclui valores diretamente em perfil/disco, logs ou IA; a página pode ler e expor seus valores no DOM.

## Lacunas e evidência

Sem localStorage, cookies, IndexedDB, Cache Storage, workers, offline, eventos storage, named properties/enumeração ou Storage.prototype/DOMException completos. Não declara compatibilidade geral com sites ou Web Storage completo.

A especificação contém roteiro-fonte futuro, com recusa de file e HTTP(S) somente onde autorizado pelo broker. Nenhum servidor foi criado/iniciado. Nenhum teste, fixture, aplicativo, navegação, Clippy, build ou comando de checagem foi executado nesta etapa documental. Revisão textual final específica do item 11 concluída sem bloqueadores; checagem estática aprovada. dist/Noir não foi reconstruído; sem commit, push ou publicação.

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. A sessão 34936 terminou com exit 0 em 3,03 s; permaneceu apenas o warning preexistente de Stmt::Function nunca construída. A revisão textual final do código foi concluída sem bloqueadores.

Testes-fonte foram apenas compilados, não executados. Os casos-fonte da shell cobrem predicados e estado; não demonstram uma operação concorrente real executada. A documentação distingue o budget VM preservado do contador host read_work restaurado pelo rollback, com scripts desabilitados na falha fatal. Não foram executados testes, aplicativo, fixtures, servidor, navegação, Clippy ou validação visual/funcional. A checagem não é build de distribuição nem conformidade Web. Itens 1–11 permanecem ABERTOS/parciais; item 19 adiado.
