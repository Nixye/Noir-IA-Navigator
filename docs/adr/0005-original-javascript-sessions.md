# ADR 0005 — VM JavaScript original e sessões retidas

Status: aceito para o subconjunto inicial. Data: 2026-11-01.

## Decisão

Implementar lexer, AST e interpretador próprios em `web-javascript`, com um contrato Host que não contém I/O. `web-bindings` oferece objetos DOM sem acesso aos serviços privilegiados. Scripts externos chegam como `ScriptAsset` aprovado pelo broker, preservando a ordem dos elementos no documento.

O renderer mantém `RenderSession`, que possui a `DocumentSession` e o VM associado. Reflow, valores de controles, eventos e ticks de timers continuam no mesmo processo/sessão. Identificadores DOM não são renumerados ao produzir uma display list; nós desconectados são excluídos da cópia de pintura para que estilos removidos não afetem a cascata.

O protocolo troca dados de controles e eventos, nunca comandos JavaScript gerados pela interface. O conteúdo de um campo é atualizado diretamente no objeto DOM antes do evento, com limite de 32 KiB por controle.

## Erros e limites

Parsing completa antes da execução. Recursos ausentes e sintaxe não suportada geram diagnósticos explícitos. Execução tem orçamentos de instruções, chamadas, strings, bindings, objetos, nós e trabalho DOM, além dos limites do processo. Falhas de script/callback restauram estado da operação, preservando instruções consumidas.

Essa política transacional não pretende reproduzir toda a semântica de erros do ECMAScript/HTML. A arquitetura permite ampliar o conjunto sem reutilizar outro motor, mas compatibilidade precisa ser demonstrada por testes específicos e, futuramente, Test262/WPT.

## Timers

O VM não cria threads ou timers do sistema. `setTimeout` registra uma tarefa lógica, e o renderer expõe o próximo prazo. O supervisor entrega ticks limitados; um lote executa até 32 tarefas prontas e não absorve recursivamente as tarefas criadas pelos callbacks do mesmo lote. As políticas da interface para espaços em segundo plano permanecem explícitas.

## Consequências

O produto já possui execução inicial e interatividade reais para o subconjunto documentado. Não pode anunciar compatibilidade geral com aplicações JavaScript, frameworks modernos ou TypeScript bruto. Módulos, Promises/microtasks, scheduler completo, APIs Web e semântica ECMAScript integral continuam pendentes. Consulte [o alcance implementado](../specifications/javascript-subset.md).
