# ADR 0023: Scripts clássicos dinâmicos mediados pelo broker

Este ADR descreve o primeiro recorte externo. O [ADR 0024](0024-synchronous-dynamic-inline-scripts.md) acrescenta inline síncrono e completa currentScript para as avaliações clássicas e seus checkpoints, sem ampliar I/O.

## Decisão

O IPC v9 acrescenta pedidos `DynamicScriptRequest { id, src }` na resposta do renderer e conclusões `DynamicScriptResponse { id, source }` na operação `Scripts`. `source: None` comunica falha sem revelar caminhos, mensagens do transporte ou dados privilegiados. Os crates `web-*` continuam sem leitura de arquivos, cliente HTTP ou resolução DNS.

A sessão retida captura o URL inicial e, para arquivos locais, o diretório canônico desse documento antes de aceitar qualquer pedido. Nenhum campo fornecido pelo renderer pode trocar essa autoridade. Cada resposta precisa passar pelas validações de protocolo, operação, display list, storage e pedidos antes de I/O. IDs de pedidos são não zero, únicos e gastos inclusive nas falhas. Conclusões só executam se correspondem exatamente ao lote pendente e à sua ordem; replay, lote parcial e reordenação são recusados antes de efeitos.

O supervisor resolve os pedidos com o broker e retorna bytes à mesma VM, sem reiniciar scripts iniciais. Navegação/fechamento descarta o worker e a autoridade; os controles transacionais já existentes impedem uma resposta de navegação antiga de substituir a aba atual.

## Política e limites

- 32 tentativas dinâmicas por documento, URL de até 8192 bytes, 128 KiB por fonte e 512 KiB de fontes dinâmicas acumuladas. Esses limites são adicionais aos iniciais, não renovam instruções/heap/DOM.
- Coleções limitadas durante a desserialização e validação semântica antes de I/O/execução. O frame mantém o teto de 48 MiB.
- Rede exige sandbox remota verificada. Reutiliza validação HTTP(S) público, portas 80/443, recusa de credenciais, DNS público fixado, recusa de downgrade/mixed content, até cinco redirects e MIME JavaScript. Não herda proxy, cookies ou Referer. Tempo de rede dinâmico é cumulativamente limitado a 25 segundos por sessão; tempo IPC não se renova a cada script de uma mesma operação.
- Arquivos só aceitam `.js` relativo, UTF-8 e caminho canônico dentro do diretório original, incluindo recusa de escapes por symlink. Documentos internos/sintéticos não ganham I/O.
- Falhas ordinárias de JavaScript preservam os efeitos anteriores. Uma fonte carregada com erro de execução ainda emite `load`, com diagnóstico/window.onerror separado; falha de carga emite `error`. Exaustão de recursos restaura a operação e desabilita execução, callbacks e pedidos pendentes.

## Subconjunto e limites de compatibilidade

O recorte carrega scripts clássicos externos criados por `document.createElement('script')`, com `src` atribuído antes ou depois de inserir o nó. Inserção de subárvore, `insertBefore` e `replaceChild` passam pelo mesmo preparo. O src é capturado uma única vez: removê-lo, trocá-lo ou mover o nó após iniciar a carga não troca a fonte nem repete a tentativa. Clones de scripts já iniciados e scripts vindos do parser/innerHTML não são reativados; scripts dentro de template/noscript ficam inertes. Clones de scripts dinâmicos ainda não iniciados podem carregar separadamente.

`src`, `type`, `text`, `async`, `defer` e `noModule` têm reflexão no elemento script. A resolução do src usa o URL real inicial, sem autoridade de `<base>` mutável. `document.currentScript` identifica o script dinâmico durante sua avaliação; não é implementação completa para scripts iniciais ou checkpoints de microtasks.

As cargas são serializadas e as conclusões executam em ordem de inserção, depois da operação JavaScript atual. `async=false` conserva essa ordem; a reflexão `async=true` não promete paralelismo. Não há parser bloqueante incremental, coordenação de `DOMContentLoaded`/`window.load` com esses novos pedidos, scripts inline dinâmicos, módulos dinâmicos, `import()` com rede, import maps, CSP/SRI completos ou compatibilidade declarada com Google/YouTube. Scripts inline/módulos dinâmicos são diagnosticados explicitamente quando preparados.

O modo CLI one-shot e replay continuam sem novas buscas: retornam aviso quando encontram pedidos dinâmicos. Capture não executa JavaScript. A fixture local deve ser aberta na sessão desktop para carregar as fontes; CLI one-shot é uma prova de ausência de I/O adicional, não de carregamento.

## Evidências

Testes de bindings verificam efeitos reais, load/error, ordem, src capturado, nós desconectados, clones/inércia, encadeamento, replay/lotes inválidos, quotas e rollback fatal. Testes de broker verificam limites cumulativos, URLs internas, extensão, traversal e symlinks. Testes de subprocesso usam arquivos locais reais e a mesma sessão, com isolamento remoto Linux fechado. A validação remota HTTP/Windows não foi executada neste ambiente Linux.
