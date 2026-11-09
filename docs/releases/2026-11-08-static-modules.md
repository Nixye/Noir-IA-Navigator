# Roadmap: módulos estáticos do item 1

Nota histórica: o [recorte posterior de lifecycle/defer](2026-11-08-document-lifecycle.md) substitui a passagem separada de módulos por uma fila DOM compartilhada com clássicos externos defer. Async real permanece ausente. As ausências abaixo descrevem este incremento original; import dinâmico registrado e import.meta foram acrescentados por entregas posteriores, sem carregamento sob demanda ou TLA. A checagem histórica desta página não valida o item 4.

Data: 2026-11-08. Estado: **implementação parcial, sem testes executados**.

Este incremento segue o [primeiro recorte JavaScript](2026-11-08-javascript-roadmap-item1.md). O item 1 permanece em andamento; não representa conformidade ECMAScript completa.

## Implementação

- Parser separado de módulos: imports default, nomeados, namespace e por efeito colateral; exports de declarações, default, listas, reexports nomeados, estrela e namespace. Import/export só são aceitos no contexto próprio.
- Registro de fontes por URL canônica. A VM recebe bytes previamente autorizados, nunca abre rede ou arquivos.
- Link do grafo inteiro antes de executar corpos, escopos separados e estritos, declarações antecipadas, TDZ, imports imutáveis e bindings vivos. Erros de nomes ausentes/ambíguos são diagnosticados no link.
- Ciclos com componentes fortemente conexos, execução única e cache de conclusão/falha. Falhas de link revertem o estado interno parcial. Namespace e escopos permanecem enraizados para o coletor.
- Namespaces limitados: objetos de protótipo nulo, não extensíveis, com getters vivos não configuráveis. Não são a implementação completa do objeto exótico de namespace ECMAScript.
- Scripts de módulo iniciais são preparados e executados após a passagem dos clássicos e antes de DOMContentLoaded. `nomodule` é ignorado; `async` em módulos é diagnosticado sem execução, pois o agendamento assíncrono ainda falta.
- Broker descobre dependências estaticamente, evita repetição/ciclos e mantém orçamento compartilhado com scripts clássicos. Locais `.js`/`.mjs` ficam dentro da raiz canonicalizada autorizada. Remotos exigem a mesma origem, MIME JavaScript e não seguem redirecionamentos de módulos para outra URL.
- O campo `ScriptAsset.src` usa `noir-module:` mais a URL canônica somente para fontes autorizadas como módulo. Esse marcador é dado interno, não um esquema navegável. O renderer separa essas fontes dos clássicos; uma resposta clássica com política menos restritiva não pode satisfazer um import.

## Limites

- Broker: quatro tentativas/arquivos externos no conjunto de scripts, 128 KiB por fonte e 256 KiB entregues. Descoberta limitada a 64 URLs; prazo de rede existente preservado. O marcador conta no limite de 8192 bytes da chave IPC.
- VM: 64 módulos, 128 KiB por fonte, 512 KiB no registro, profundidade 64, até 256 entradas de imports/exports/requests por módulo e 128 exports no namespace. Orçamento de execução e memória continua cumulativo.
- Resolução suporta URLs e caminhos relativos/absolutos. Bare specifiers exigem import maps, ainda ausentes. URLs com credenciais são recusadas.
- Ainda ausentes: importação dinâmica, import.meta, import maps, top-level await, avaliação assíncrona, import attributes e módulos JSON/Wasm. Não há novas permissões remotas, CORS amplo, requisições iniciadas pela VM ou cache persistente.
- Scripts dinâmicos e o agendamento completo async/defer são trabalho posterior. Falta comprovar comportamento funcional e compatibilidade em sites.

## Verificação e entrega

`cargo check --workspace --all-targets --offline --locked` passou durante a integração. Essa checagem compila fontes, inclusive testes, mas não os executa e não comprova o comportamento em execução.

Foram escritos testes unitários de parser, grafo, ciclos, bindings vivos, falhas, budgets, origem e separação de assets, **sem execução**. A fixture `fixtures/javascript-modules/index.html` e seus três módulos também não foram executados.

Não houve cargo test, Clippy, Test262, WPT, navegação, build release, empacotamento, commit, push ou publicação. `dist/Noir` permanece inalterado. Veja também [ADR 0014](../adr/0014-brokered-static-modules.md).
