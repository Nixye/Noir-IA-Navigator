# Recovery-r4: inicialização de listas por NodeList.forEach

2026-11-12, conclusão da autoria antes da validação. Resultados e pins posteriores
pertencem ao relatório externo do checkpoint; esta nota não antecipa PASS.

R4 continua a base V13 restaurada via R1/R2/R3, sem recuperar V14–49. O
[recorte NodeList](../specifications/nodelist-foreach-recovery-r4.md) unifica item
para snapshot/viva e compartilha forEach com a implementação genérica da VM.
Comprimento capturado e leituras atuais por índice preservam os efeitos reais
de mutações durante callbacks; snapshots continuam estáticos.

Fixture local inicializa filtros, registra eventos e aplica classes via forEach,
com thisArg/índices/receiver, depois Promise, RAF e click. Oráculos escritos
verificam DOM, cores/geometria de cartões, processo retido e ausência de replay.
A/B com baseline R3 só será evidência depois de execução autorizada separada.

A limitação R3 de snapshot sem item foi intencionalmente ampliada. Um teste R3
do núcleo será migrado com registro; não se declara preservação integral de
todos os oráculos antigos. Documentos R3 seguem como registros históricos,
não são reescritos como se já prometessem esta capacidade.

Genericidade é limitada pelo boxing existente (String127 unidades UTF-16,
sem boxed Symbol), orçamentos VM/DOM e ausência de iteradores/protótipos
completos. Não há nova autoridade de rede/arquivo, dependência, IPC ou segurança.

Manifestos herdados mantêm seu contexto; o novo manifesto externo e validação
R4 estavam pendentes ao concluir autoria. Sem alegação de GUI/Windows/Google/
sites públicos/WPT/enforcement ou implementação ECMA/Web completa.
