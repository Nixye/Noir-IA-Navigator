# Eventos: subconjunto do primeiro recorte do item 5

## Estado atual: validação e frames — 2026-11-08

A instrução posterior autoriza testes e ciclos de correção. Item 19: **validação em andamento**; itens 1–18 continuam abertos/parciais. O [baseline funcional](../validation/2026-11-08-functional-baseline.md) registra comandos e resultados novos. As seções anteriores desta rodada abaixo são históricas: o adiamento e a ausência de testes valiam naquelas entregas e não são reescritos como sucesso funcional.

O [segundo recorte do item 4](../releases/2026-11-08-animation-frames.md) acrescenta requestAnimationFrame/cancelAnimationFrame limitados, snapshot de até 32 callbacks, tempo real do host e IPC v8. A integração desktop foi liberada, com revisão consolidada sem bloqueadores e checagem estática aprovada; testes de frames e validação funcional continuam pendentes. Não equivale a event loop Web completo, vsync ou apresentação GPU. O contrato de fronteira consta no [ADR 0021](../adr/0021-host-animation-frame-pulses.md).


Estado: implementação parcial, revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. Itens 1–5 ABERTOS/parciais; item 19 adiado. Este contrato não descreve interfaces DOM/Web IDL completas.

## Construção e identidade

Event e CustomEvent são construtores host limitados, acessíveis globalmente e por window. `new Event(type, init?)` cria um Event e `new CustomEvent(type, init?)` acrescenta detail. Bubbles e cancelable são false por padrão; detail é null quando omitido e conserva o valor JavaScript original quando fornecido, sem serialização ou clone. Referências de detail participam das raízes do GC enquanto o evento estiver retido. Detail explicitamente undefined também usa o default null; Event comum não expõe detail.

Type é obrigatório: omiti-lo produz erro; undefined explícito passa pela conversão ToString observável e se torna "undefined". A conversão executa os hooks suportados da VM, recusa Symbol e surrogates isolados, e limita o resultado UTF-8 a 64 bytes. Tipo vazio pode ser construído, mas dispatchEvent o recusa.

Init omitido, undefined ou null equivale a dicionário vazio. Outros primitivos são recusados; objetos representáveis usam a leitura real de propriedades, inclusive getters, na ordem bubbles, cancelable, composed e, somente em CustomEvent, detail. Bubbles/cancelable/composed usam truthiness. Composed truthy é explicitamente recusado antes de ler detail; composed exposto no evento é false. Não há conversões Web IDL completas.

A chamada sem new é recusada. Typeof retorna function; name distingue Event/CustomEvent e length é 1. New de função vinculada e Reflect.construct usam a ponte de construção limitada; newTarget omitido ou idêntico é admitido, newTarget diferente é recusado. Prototype, instanceof e class extends para esses hosts continuam explicitamente não suportados. Uma função host chamável não se torna construtível por acidente: construção é opt-in.

## Dispatch síncrono

Window, Document e Node oferecem dispatchEvent para eventos genuínos retidos nesta sessão; um objeto comum com propriedades semelhantes não basta. Métodos lidos como valores exigem receptor compatível. O método executa os listeners sincronamente e retorna false somente se o evento for cancelável e tiver sido cancelado. Não dispara navegação, envio de formulário, foco ou outra ação padrão nativa.

O caminho é fixado antes dos callbacks: nós conectados usam ancestrais reais, Document e Window; nós destacados usam somente a cadeia real de pais, sem ligação artificial ao Document por ownerDocument. Document inclui Window e Window usa apenas a si. Captura vai dos ancestrais externos ao alvo; no alvo, captura e não captura têm eventPhase 2; bubble percorre ancestrais somente quando bubbles é true. Bubbles/cancelable dos eventos construídos vêm do init, não de uma tabela de tipos nativos.

Callbacks recebem o mesmo evento e this igual a currentTarget. Target inicia null e passa a identificar o alvo do dispatch. Ao terminar, currentTarget volta a null e eventPhase a zero. É permitido reutilizar posteriormente o mesmo evento, inclusive em outro alvo, preservando detail e cancelamento. Os flags de parada são limpos ao terminar, mas defaultPrevented persiste. IsTrusted é false na construção e em redispatch por JavaScript. Disparar novamente o próprio evento enquanto ele está em propagação é recusado; dispatch aninhado de outro evento está sujeito aos limites comuns.

PreventDefault só cancela eventos canceláveis fora de listener passive. StopPropagation conserva os demais listeners do grupo atual, mas impede os próximos grupos, inclusive o grupo não-capture (bubble) no alvo após o grupo capture do alvo. StopImmediatePropagation também interrompe os demais callbacks do grupo atual. Se a parada foi marcada antes de dispatchEvent, nenhum grupo executa; a limpeza ao terminar permite reutilizar posteriormente o evento. CancelBubble/returnValue, once e remoção de listeners seguem o subconjunto anterior. Once é removido antes da invocação; listeners removidos durante a propagação são ignorados. Novos listeners não entram no snapshot já iniciado da fase atual.

## Falhas, microtasks e recursos

Erros ordinários de listeners são registrados no log limitado e notificados a window.onerror quando presente; os demais listeners continuam. O handler mantém o contrato anterior de cinco argumentos, sem recursão para seu próprio erro ordinário. O erro não se torna automaticamente uma exceção do chamador de dispatchEvent. Efeitos ordinários já realizados permanecem, conforme [ADR 0007](../adr/0007-script-error-semantics.md).

Resource continua fatal e não capturável; alcança a fronteira transacional exterior que contém a chamada (execução de script/módulo, callback de timer ou callback nativo), restaura DOM/VM/estado dessa operação preservando consumo e desativa scripts. Não promete desfazer a inicialização inteira nem callbacks nativos anteriores: a entrega nativa mantém sua fronteira por callback. Dispatch por JavaScript não cria uma transação por listener nem intercala checkpoints extras de microtasks entre listeners. A fila drena nos checkpoints exteriores existentes, não antes de dispatchEvent retornar.

Até 256 eventos são retidos por sessão, incluindo eventos iniciais/nativos e construídos, sem reciclagem individual. Até 128 listeners simultâneos; tipos têm até 64 bytes UTF-8. Dispatch limita aninhamento a 32. O caminho respeita MAX_DEPTH do DOM (128) durante a subida de pais; Window é acrescentado somente após alcançar Document. Heap, instruções, trabalho DOM e profundidade de chamada permanecem compartilhados e podem limitar antes. Não há novos recursos de rede, I/O, IPC ou permissões do renderer.

## Fora do recorte

EventTarget independente, eventos especializados, protótipos e descritores DOM completos, instanceof/subclasses host, objetos handleEvent, AbortController/AbortSignal, Shadow DOM e composedPath, teclado/ponteiro/toque/IME completos, foco/seleção/ranges, contenteditable, Clipboard e drag-and-drop permanecem ausentes ou parciais. Um nome de evento não cria integração com o sistema operacional ou com ações padrão nativas.

A [fixture programmatic-events.html](../../fixtures/programmatic-events.html) contém ordem esperada, once/passive, cancelamento, reentrada recusada, detail, reuso destacado e microtask para interação posterior. É fonte não executada, não evidência de aprovação. Veja a [entrega](../releases/2026-11-08-programmatic-events.md).
