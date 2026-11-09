# Frames de animação limitados: segundo recorte do item 4

Data: 2026-11-08. Estado: implementação parcial com fontes liberadas; contrato verificado nas fontes, validação funcional pendente. A execução nova de testes é acompanhada no [baseline](../validation/2026-11-08-functional-baseline.md), sem reclassificar os históricos anteriores.

## Callbacks e cancelamento

requestAnimationFrame(callback) e cancelAnimationFrame(handle) são funções host de Window, também expostas nos aliases globais existentes. O callback deve ser callable. O argumento é obrigatório nas duas APIs; receptores diferentes de Window/null/undefined são recusados. Não executam strings como código.

A fila admite até 32 callbacks pendentes por DocumentSession. Cada pedido recebe um inteiro positivo seguro, monotônico até 2^53−1; identidades consumidas não voltam com rollback. Esgotar fila ou handles produz Resource, não wraparound.

DocumentSession::animation_frame_pending informa trabalho pendente e run_animation_frame(timestamp_ms) executa um snapshot fixo dos IDs na ordem de registro. Os callbacks continuam enraizados para GC enquanto pendentes. Um callback pode cancelar outro ainda não invocado no lote; pedidos feitos durante o lote ficam para uma oportunidade posterior. Cada callback recebe o mesmo timestamp do lote, um único argumento e Window como this, com o checkpoint de jobs existente após a chamada.

cancelAnimationFrame faz ToNumber observável. Apenas inteiros finitos positivos seguros podem remover IDs pendentes; frações, negativos, NaN, infinitos, zero e IDs desconhecidos são no-op. Erros da coerção são preservados. Isso não implementa a conversão unsigned long com wrap de navegadores completos.

Erros ordinários seguem onerror e não impedem os próximos callbacks do snapshot. Resource restaura o checkpoint anterior ao callback que falhou, desabilita scripts e limpa o trabalho pendente; callbacks anteriores já concluídos não são desfeitos. O tempo aceito permanece fora do estado restaurável.

## Tempo, renderer e IPC v8

SessionRequest::AnimationFrame transporta id, timestamp_ms e viewport. RenderedPage exige animation_frame_pending; a alteração usa protocolo 8, sem compatibilidade silenciosa com mensagens v7. Supervisor, renderer e bindings recusam tempo não finito, negativo, acima de 2^53−1 ms ou menor que o último aceito. Timestamps iguais são permitidos. A aceitação ocorre antes de callbacks e não retrocede com rollback.

RenderSession::animation_frame executa os callbacks antes de produzir a display list, com o viewport validado. Não avança implicitamente o relógio lógico dos timers. IDs, prazo de transporte, validação de respostas, cotas e checkpoints existentes continuam aplicáveis. Nenhum acesso a rede/arquivos é introduzido no motor. Veja [ADR 0021](../adr/0021-host-animation-frame-pulses.md).

## Oportunidades no desktop

A integração em fontes usa o fim de App::ui de egui 0.37 para oferecer um pulso apenas à aba ativa cujo documento/sessão foi efetivamente exibido, com callback pendente. A janela não pode ser conhecida como oculta (visible != Some(false)); foco não é exigido. Pendências, fila, operação de IA, prioridade de outras operações, descarte do passe e fechamento impedem a oferta. A oportunidade é deduplicada por viewport/frame.

O timestamp vem de Instant relativo à sessão, capturado após load/seed e clone dos bytes, imediatamente antes de spawn_with_storage e instalado apenas no commit de um novo documento, não de incremento sintético. Abas inativas não acumulam pulsos para replay. Operações já enviadas não são retrocanceladas por uma mudança posterior de visibilidade. Timers vencidos têm prioridade e timers de atraso zero podem adiar frames. A oferta é descartável, não enfileirada atrás de outras operações. Não há promessa de vsync, pintura no mesmo frame, frequência fixa ou apresentação GPU.

## Evidência e escopo

Fontes de regressão estão preparadas em web-bindings, browser-ipc, browser-process e web-renderer. A presença dessas fontes não comprova execução. Na liberação inicial não havia resultado funcional deste recorte; a primeira execução parcial posterior está registrada ao final desta nota e no baseline. O autor liberou shell.rs e confirmou o contrato; revisão consolidada concluída sem bloqueadores. Checagem central aprovada: cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked; sessão 9828, saída 0, 2,52 s informados pelo executor. Esses comandos não executam testes de frames; validação funcional continua pendente.

Item 4 permanece aberto: este recorte não implementa event loop Web completo, parser incremental, scheduler completo, performance timeline, visibilidade Web completa ou compatibilidade geral. Item 19 está em validação por instrução posterior; o adiamento registrado nas releases anteriores continua sendo histórico verdadeiro.

## Primeira execução parcial posterior

O baseline independente executou browser-ipc: 22 aprovados, 1 falha em animation_frame_timestamp_bounds_and_request_round_trip, na igualdade do timestamp após JSON. A causa segue em investigação. Não houve execução confirmada de callbacks rAF ponta a ponta ou desktop neste registro. Esse resultado posterior não altera a natureza estática da checagem anterior; veja ciclo 7 do [baseline](../validation/2026-11-08-functional-baseline.md).


## Correção posterior de precisão IPC

A causa foi reproduzida: JSON 9007199254740991.0 perdia uma unidade ao decodificar. Habilitar serde_json float_roundtrip no workspace corrigiu o round-trip, sem nova dependência ou mudança de Cargo.lock. Browser-ipc passou 23/23; os testes de frames browser-process passaram 2/2 e web-renderer 3/3, com outros casos filtrados nesses dois grupos. Logs ipc-float-before.log, ipc-float-after.log e ipc-float-temporal.log em target/validation. Ainda não é validação do desktop/GPU ou compatibilidade Web completa; veja ciclo 10 do baseline.
