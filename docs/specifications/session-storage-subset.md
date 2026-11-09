# Subconjunto sessionStorage — item 11

Estado: implementação parcial, sem conformidade Web Storage declarada. Revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada; sem validação funcional. Veja a [fronteira IPC](../adr/0017-bounded-session-storage.md).

## Escopo e identidade

sessionStorage e window.sessionStorage expõem a mesma identidade host estável, somente leitura. O conteúdo é real, em memória, pertencente à aba e particionado pela origem HTTP(S) canônica da URL final autorizada pelo broker. Esquema, host e porta efetiva compõem a origem; caminho, consulta e fragmento não criam buckets. O renderer não fornece a origem.

file:, noir:, data:, origem opaca ou URL ausente são recusados com erro ordinário no getter/método; não existe bucket null compartilhado. Refresh, histórico, A→B→A e HOME/SETTINGS não apagam os buckets da aba. Outras abas/workspaces não compartilham valores. Fechar a aba ou reiniciar elimina o estado; perfil, URLs restauradas e abas fixadas não o recuperam.

## Operações e conversões

- length: número de pares do bucket corrente.
- key(index): chave na posição de inserção; ausência retorna null.
- getItem(key): valor associado ou null quando ausente.
- setItem(key, value): insere ou sobrescreve após conversões e limites.
- removeItem(key): remove somente a chave indicada; ausência é no-op.
- clear(): remove todos os pares do bucket corrente.

O receiver deve ser exatamente o objeto sessionStorage. Aridade mínima: key/getItem/removeItem exigem 1 argumento, setItem exige 2 e clear exige 0. Argumentos extras são ignorados. A aridade é verificada antes das coerções. Não é uma implementação integral de Web IDL.

Sobrescrita conserva posição; remoção/reinserção vai ao fim. setItem faz ToString observável da chave e depois do valor via Runtime. Surrogates isolados são recusados na fronteira UTF-8 sem substituição silenciosa. Erros ordinários das coerções não viram sucesso. key usa ToNumber observável com hint number; Symbol e BigInt causam erro. Não finitos e zero viram 0; finitos são truncados e reduzidos módulo 2^32.

## Quotas e falhas

| Recurso | Limite |
| --- | --- |
| Pares por origem | 128 |
| Chave UTF-8 | 1024 bytes (1 KiB) |
| Valor UTF-8 | 16384 bytes (16 KiB) |
| Soma de chaves e valores/origem | 65536 bytes (64 KiB) |
| Buckets retidos/aba | 16 |
| Trabalho host cumulativo | 2.000.000 unidades |

A soma usa aritmética verificada. Quota de dados gera erro ordinário capturável Session storage quota exceeded, sem DOMException completa. setItem recusado não deixa inserção parcial nem remove valor anterior. Efeitos anteriores à exceção permanecem conforme ADR 0007. Orçamentos de trabalho host e coerção VM são separados: Resource é fatal e restaura a operação, inclusive storage, conservando orçamento VM consumido e desabilitando execução futura.

Snapshot vazio de origem nova não ocupa slot; limpar bucket existente não libera slot. Sem eviction. Com 16 slots ocupados, origem nova recebe QuotaExceeded: length zero, leituras vazias, setItem gera erro capturável, removeItem/clear são no-op. Origens existentes continuam acessíveis.

## Transporte e confirmação

IPC v5 transporta somente seed/snapshot corrente. O seed é instalado uma vez antes dos scripts, após definir URL HTTP(S); divergência posterior de metadados é recusada. Validação ocorre nos dois extremos, incluindo duplicatas, quotas e acesso/entradas. O supervisor confirma snapshot somente depois da resposta inteira válida, associada à sessão e requisição corretas. O renderer não troca bucket por declarar origem/identidade no payload.

Navegação inicial superada não confirma escritas. Operação em andamento de sessão já comprometida pode confirmar checkpoint da origem antiga, sem publicar página obsoleta, antes da próxima navegação enfileirada. Crash, falha ou timeout mantém último checkpoint confirmado, não alterações pendentes. O estado do engine participa do rollback.

Frame máximo: 48 MiB. JSON é desserializado antes das quotas semânticas menores de storage. Não há promessa de toda alocação transitória caber em 64 KiB nem de ausência de cópias. Nenhum I/O de rede/arquivo é acrescentado ao engine.

## Fora do recorte

Sem Storage.prototype/DOMException completos, named properties (storage.foo), enumeração dos pares como propriedades, eventos storage, localStorage, cookies, IndexedDB, Cache Storage, workers ou offline. Sem disco, sincronização entre abas ou restauração após reinício. O navegador não anexa storage diretamente ao perfil, logs ou IA; a página pode ler e mostrar seus próprios valores no DOM.

## Roteiro-fonte futuro, não executado

Use somente página HTTP(S) em destino permitido pelo broker. HTML local por file: deve recusar storage. Este roteiro não pressupõe criar servidor, liberar loopback/rede privada ou alterar sandbox.

1. Conferir identidade global/window, inserção, sobrescrita, remoção/reinserção, ordem, length, ausência null, receiver e aridade.
2. Recarregar, navegar entre caminhos da mesma origem, seguir a outra origem autorizada e voltar. Conferir retenção por origem e separação entre abas/workspaces.
3. Visitar HOME/SETTINGS e retornar. Fechar/reabrir e reiniciar: não esperar recuperação.
4. Exercitar fronteiras de pares/bytes, sobrescrita no limite, coerções com efeitos observáveis e surrogate isolado. Capturar erros ordinários e conferir atomicidade; distinguir Resource fatal.
5. Exercitar 16 origens, bucket existente limpo e origem nova vazia. Sem eviction; a 17ª origem lê vazio e recusa setItem.
6. Em futura instrumentação controlada, conferir IPC inválido, duplicatas, versão/ID, navegações sobrepostas, crash/timeout e rollback. Uma página HTML isolada não demonstra essas fronteiras.

São expectativas do contrato-fonte, não observações de execução. Não foi criada/executada fixture HTML nem iniciado servidor.
