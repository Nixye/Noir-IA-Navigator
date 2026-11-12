# Continuação V13: classList recovery-r1

Estado registrado em 2026-11-12, na conclusão da autoria, antes da validação:
AUTORIA LOCAL, NOT_RUN. Resultados posteriores constarão no relatório externo
do checkpoint, vinculados às fontes efetivamente validadas.

A base restaurada V13 contém 1061 arquivos verificados na cópia e no ZIP de
origem. Esta é uma continuação nova dessa base, não restauração de V14–49 e
não herda resultados de versões perdidas. O suporte novo é o subconjunto
DOMTokenList/classList descrito no [contrato de escopo](../specifications/classlist-recovery-r1.md).

Cenário útil: menu, aba e tema usam uma lista de classes retida em vez de
reescrever manualmente className. A fixture inline muda CSS/layout no frame,
fecha o menu por RAF e reabre por click. Testes novos verificam DOM vivo,
coerção, atualização atômica, estado retido, PID estável e ausência de replay.
A conversão String(classList) usa Host::stringifier opt-in, padrão None,
habilitado por DomHost somente para CLASS, com cobrança de leitura DOM e
trabalho/string pela VM. Não abre coerção arbitrária de outros objetos host.
Nenhum fallback de rede ou alteração do guard HTTP/HTTPS foi introduzido.

Na conclusão da autoria em 2026-11-12, antes da validação, não haviam sido
executados neste bloco Cargo, formatter, testes, aplicativo, GUI, Windows ou
navegação em sites públicos. As 29 declarações de testes novos, incluindo
5 de integração, são inventário de autoria, não contagem de resultados.
Nenhum resultado funcional é alegado aqui. A validação posterior terá evidência
local no relatório externo do checkpoint; autoria não equivale a aprovação
dos gates nem a compatibilidade completa com a Web.

## Proveniência

SOURCE-MANIFEST.json herdado descreve um snapshot histórico de 933 arquivos,
não o conjunto V13 restaurado de 1061 arquivos nem esta continuação.
docs/source-snapshot-2026-11-09.md também é histórico. Ambos permanecem
preservados e não são apresentados como validação das fontes atuais.

Na conclusão da autoria em 2026-11-12, o novo manifesto externo do checkpoint
e seus resultados/pins estavam pendentes de geração e verificação. Consulte o
relatório externo do checkpoint para o estado posterior. Não há pacote novo
ou publicação alegados neste registro de autoria. As seções antigas de README/matriz continuam a
registrar seu contexto histórico, sem reclassificação retroativa.

## Limitação preexistente de pintura observada no diagnóstico local

Em 2026-11-12, após a primeira tentativa de testes, duas renderizações locais
com binário existente mostraram fundo da caixa e fundo do fragmento textual
na mesma cor: caixa 120×24 e texto Menu 40,816×24; no controle estático sem
JavaScript/classList, caixa 40×24 e texto Box 27,376003×24. O segundo retângulo
coincide com a geometria do texto, não representa outra caixa de layout.
As fontes CSS/layout desse caminho são idênticas à base V13 preservada.

Os oráculos de integração distinguem a caixa pela cor e dimensões completas,
exigem exatamente uma caixa esperada e recusam qualquer fundo da cor oposta.
Não pressupõem quantidade total de primitivas por cor. A sobrepintura do fundo
do texto é uma limitação preexistente separada, não corrigida por classList.
Não foi testada composição alpha/transparência; esse diagnóstico opaco não
comprova sua correção. Resultados da nova tentativa pertencem ao relatório
externo do checkpoint, não são antecipados por esta correção de oráculo.
