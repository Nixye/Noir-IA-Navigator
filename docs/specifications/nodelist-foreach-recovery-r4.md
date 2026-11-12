# NodeList.item e forEach: recovery-r4

Registro de autoria em 2026-11-12, antes da validação. Resultados posteriores
constarão no relatório externo do checkpoint, ligados à fonte efetivamente
testada. R4 continua a V13 restaurada via R1/R2/R3; V14–49 não foram recuperadas.

## Superfície

NodeList.item é unificado entre o snapshot de querySelectorAll e a coleção
childNodes viva. Isso amplia deliberadamente a limitação documentada em R3:
o snapshot já não fica restrito a length/índices sem item. Essa ampliação tem
migração explícita de um oráculo R3 no núcleo; não se alega que todos os testes
históricos permanecem intocados. Os demais recortes anteriores são preservados.

NodeList.forEach expõe o mesmo Native de Array.prototype.forEach, generalizado
na VM; não há segundo loop exclusivo no binding. HTMLCollection não possui
forEach próprio, mas o empréstimo do método genérico funciona. Não se acrescentam
Symbol.iterator, iteradores, protótipos completos ou outras APIs NodeList.

## Ordem observável

O comprimento é lido/converso e capturado uma vez. Para cada índice até esse
limite, HasProperty e Get observam o estado atual. Callback recebe
(value, index, receiver), com thisArg fornecido. Callback inválido falha mesmo
com lista vazia; throw interrompe a iteração. Ausência de uma propriedade pula
o callback, mas não permite pular trabalho de orçamento por índice.

Adicionar após o comprimento capturado não estende a visita. Remover/reordenar
uma coleção viva pode pular ou revisitar membros; append combinado com remoção
pode trazer um nó novo para um índice ainda dentro do limite. Snapshot mantém
os nós capturados, mesmo removidos/reparentados no DOM. Nenhuma atualização de
membros de querySelectorAll é introduzida.

## Limites de genericidade

Null e undefined são recusados como receiver. Outros receivers usam o boxing
existente: wrapper String limitado a 127 unidades UTF-16; boxed Symbol não é
implementado. ToLength limita a 2^53−1, mas cada índice consome o orçamento de
instruções existente. Não há novo cap nem aumento de orçamento; comprimento
enorme não implica execução ilimitada. Isso não equivale a ECMA/Web IDL completo.

Quotas DOM de R3 permanecem:64 descritores vivos/64 snapshots separados,4096
membros por snapshot,2.000.000 unidades de leitura,20.000 nós/profundidade128.
Não há novos IPC/dependências/origens, autoridade de rede/arquivo ou mudança
de segurança. Integração renderer/CLI altera somente testes e registros.

## Fixture e oráculos escritos

nodelist-foreach-recovery-r4.html inicializa filtros por querySelectorAll(...).
forEach é a primeira capacidade nova exercitada, antes de item, para que um
A/B posterior com R3 identifique a ausência real dessa API, não uma falha
incidental. Callback usa índices, receiver e thisArg para registrar handlers e
classes. Promise/RAF/click exercitam seleção e estado retido sem replay.

Renderer/worker observam cores e geometria de cada cartão240×48, título,
contadores e PID/reflow/pulso ocioso, sem cardinalidade global de primitivas.
Outro renderer contrasta mutação de childNodes durante iteração com snapshot
preservado e prova que append além do limite capturado não ganha callback.
Entrada sintética inline usa noir:, sem acesso remoto/file.

Na autoria, testes/gates ainda não foram executados. Não há alegação de GUI,
Windows, sites públicos, Google, WPT, ECMA completo ou enforcement validado.
