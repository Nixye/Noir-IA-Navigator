# Diagnóstico de carga dinâmica — 2026-11-09

Uma fonte ausente na conclusão do broker agora produz um aviso de `phase=load`, separado de falhas de avaliação JavaScript e do callback `error`. Antes da correção, uma carga recusada/indisponível podia não produzir aviso algum se seu callback não falhasse. Duas regressões novas reproduziram essa ausência antes da mudança.

O aviso conserva somente request ID e NodeId: não copia URL, credenciais, parâmetros, fragmentos, caminhos nem bytes da resposta. O IPC atual não informa a causa exata de ausência, portanto o diagnóstico não inventa erro HTTP, DNS, MIME, política ou quota. Teto de 2048 bytes por aviso e quota compartilhada de 32 avisos preservados.

Não há alteração de IPC v9, rede, sandbox, carregamento, exceções JavaScript ou ordem de eventos. Carga falha continua emitindo error. Fonte recebida com exceção ainda emite load e seu diagnóstico separado. Reflow não duplica avisos. One-shot/replay continuam sem buscar recursos pendentes e não os rotulam como falha de rede executada.

## Medição de orçamentos sem aumentar quotas

Lexer, contador de construção AST e construtor Array recebem contextos exclusivamente numéricos. O vetor de tokens deixa de dobrar para 65.536 slots no limite e fica limitado a 32.769, incluindo EOF. O corte de 32.768 tokens, 16.384 visitas do parser, limites de fonte, arrays, instruções e strings permanece.

Os testes sintéticos verificam tokens, EOF, limites de fonte/AST, arrays antes de alocação e orçamento agregado. Nenhuma quota foi aumentada no produto. Contextos de diagnóstico usam apenas valores numéricos e não copiam conteúdo de páginas.

## Validação focada Linux

- `cargo test -p web-bindings -p web-renderer --locked`: 186 bindings e 16 renderer aprovados na rodada focal final, sem falhas, incluindo as mudanças concorrentes de descritores Window. Cinco regressões novas do loader cobrem privacidade, separação de fases, saturação do limite e reflow sem replay.
- `cargo test -p browser-cli --test dynamic_scripts --locked`: oito integrações aprovadas, incluindo asserções de diagnóstico para recusas reais e ausência de falso erro de rede em one-shot.
- `cargo test -p web-javascript budget_tests --locked`: nove regressões de orçamento aprovadas.
- `cargo test -p web-bindings resource_budget_context --locked`: regressão integrada de diagnóstico, rollback e desativação aprovada.
- `cargo clippy -p web-javascript -p web-bindings -p web-renderer -p browser-cli --all-targets --locked -- -D warnings`, `cargo fmt --all --check` e `git diff --check`: aprovados.

Esses resultados são focados, não uma nova validação integral do workspace nem prova de sites funcionais. Este recorte melhora o diagnóstico e limita alocações, sem ampliar o subconjunto aceito. A validação agregada está registrada separadamente.
