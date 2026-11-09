# ADR 0021 — Pulsos de animação fornecidos pelo host

Data: 2026-11-08. Estado: contrato implementado em fontes; integração desktop liberada em fontes e validação funcional pendente.

## Contexto

Callbacks de animação precisam de oportunidades reais do host sem transformar timers lógicos em frames simulados nem dar ao motor acesso ao relógio/plataforma privilegiados. O histórico de codec limitado v7 permanece em ADR 0020; esta alteração exige uma nova versão de protocolo.

## Decisão

O IPC v8 acrescenta SessionRequest::AnimationFrame { id, timestamp_ms, viewport } e torna obrigatório RenderedPage.animation_frame_pending. O supervisor envia um pulso com tempo real relativo à sessão. Renderer e bindings validam novamente os dados recebidos; o motor não abre rede, arquivos ou relógios de plataforma para atender ao pedido.

O tempo deve ser finito, de 0 a 2^53−1 ms e não decrescente por sessão; igualdade é aceita. O último tempo aceito fica fora de checkpoints restauráveis. Mensagens inválidas são rejeitadas, não corrigidas por clamp ou incrementos fabricados. Mantêm-se ids, deadlines, validação semântica e cotas de transporte existentes.

O renderer roda um snapshot limitado de até 32 callbacks antes da display list. Não dispara Tick implicitamente. Cancelamento ainda pode remover um callback do lote antes da sua invocação; callbacks novos esperam outro pulso. O host só oferece oportunidades à sessão ativa efetivamente exibida, sem replay de períodos inativos.

## Consequências e limites

Processos devem concordar com v8; o novo campo não tem default para ocultar incompatibilidade. A fronteira de confiança continua data-only: o conteúdo não escolhe um comando privilegiado, não ganha I/O e não controla o relógio. Campos de tempo e viewport continuam dados não confiáveis.

A oportunidade egui não é confirmação de vsync ou apresentação GPU; uma operação já enviada não pode ser retrocancelada por visibilidade posterior. Nenhuma nova prova de isolamento, auditoria de segurança, conformidade Web ou ganho de desempenho é alegada.

## Evidência

[Contrato detalhado](../releases/2026-11-08-animation-frames.md). [Baseline de execução](../validation/2026-11-08-functional-baseline.md). Fontes de testes e revisão textual não substituem resultados executados.