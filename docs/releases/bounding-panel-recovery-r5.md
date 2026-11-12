# Recovery-r5: painel ancorado por border-box real

2026-11-12, conclusão da autoria antes da validação. Resultados e pins posteriores
pertencem ao relatório externo do checkpoint; esta nota não antecipa PASS.

R5 continua V13 restaurada via R1–R4, sem recuperar V14–49. O
[recorte de medição](../specifications/bounding-panel-recovery-r5.md) acrescenta
getBoundingClientRect para caixas principais suportadas, com snapshot ordinário
de oito números e contexto de viewport/scroll explicitamente fornecido pelo host.

Fixture local posiciona um popup por diferenças de medições, altera CSS em RAF
e restaura por click. Oráculos escritos observam border/padding fracionários,
frame real, snapshot antigo, scroll/unknown, NoBox e retenção do worker. Scripts
pendentes recebem contexto antes da execução, sem nova autoridade de arquivo/rede.

None significa contexto desconhecido, não origem zero; NoBox retorna zeros,
mas inline/tabela/imagem decorada não suportados geram erro explícito. Clipping
e visibility:hidden não encolhem border-box. Snapshot não é DOMRect completo.
IPC10 e contexto novo não constituem promessa de isolamento validado.

Histórico R1–R4 e V13 permanece preservado. Manifestos herdados conservam seu
sentido; validação R5 e manifesto externo estavam pendentes na autoria. A/B com
baseline somente após execução autorizada separada. Sem alegação de GUI,
Windows, sites públicos, Google, WPT ou CSSOM View completo.
