# Recovery-r3: referências vivas a filhos DOM

2026-11-12, conclusão da autoria antes da validação. Resultados e pins posteriores
pertencem ao relatório externo do checkpoint; não se antecipa PASS nesta nota.

R3 é continuação nova da V13 restaurada via R1/R2, não recuperação de V14–49.
O [recorte de coleções vivas](../specifications/live-collections-recovery-r3.md)
permite guardar children/childNodes e acompanhar edições de listas sem renovar
snapshots em cada acesso. querySelectorAll continua snapshot, sem ser convertido
indevidamente em coleção viva.

Fixture local: menu/lista editável com 96 pares de leituras, Promise, RAF,
reparent, innerHTML e click. Oráculos escritos observam identidade, estado atual,
conteúdo e geometria de frames e retenção do processo sem replay. Não houve
navegação remota nem relaxamento de segurança. A/B com binário anterior só será
evidência se executado e registrado separadamente após autorização.

Quotas:64 descritores vivos/64 snapshots separados,4096 membros por snapshot,
2.000.000 unidades de leitura, DOM20.000 nós/profundidade128. Não é suporte a
todas as coleções/iteradores/protótipos DOM ou a sites Web modernos completos.

R1/R2/V13 e suas evidências permanecem preservadas. Manifestos e notas herdadas
mantêm seu sentido histórico; o novo manifesto externo R3 e resultados estavam
pendentes ao concluir autoria. Não há validação de GUI/Windows/Google/sites
públicos/WPT/enforcement alegada nesta nota.
