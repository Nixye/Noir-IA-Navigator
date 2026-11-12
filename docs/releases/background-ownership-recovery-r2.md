# Recovery-r2: fundos de caixa sem repintura pelo texto direto

2026-11-12, conclusão da autoria antes da validação. Resultados posteriores
constarão no relatório externo do checkpoint; este registro não antecipa PASS.

R2 continua a V13 restaurada via R1. V14–49 não foram recuperadas. O bloco
corrige a sobrepintura de fundo sob texto direto do proprietário de uma caixa,
sem apagar camadas inline legítimas nem alterar CSS computado. Veja o
[escopo técnico](../specifications/background-ownership-recovery-r2.md).

O cenário local alterna classes e alpha numa caixa translúcida em RAF. Testes
novos observam frames e pixels CPU, retenção do worker sem replay, camada inline
da mesma cor, posicionamento e clipping. O resultado esperado usa alpha127
para .5 e truncamento inteiro por camada, não arredondamento idealizado.

Testes R1, fixture R1, histórico e base V13 permanecem preservados. A nota R1
sobre sobrepintura descreve o diagnóstico daquele checkpoint; este bloco é uma
correção nova, não reescrita de evidência anterior. Camadas/gaps de ancestrais
inline e propagação html/body ao canvas permanecem fora do recorte.

Na autoria, validação local R2 e manifesto externo do novo checkpoint estavam
pendentes. SOURCE-MANIFEST.json e docs/source-snapshot-2026-11-09.md herdados
continuam históricos, não validam R2. Consulte o relatório externo do checkpoint
para resultados, fonte efetivamente testada e pins quando disponíveis.

Nenhum resultado de GUI/Windows/GPU/sites públicos/Google/WPT ou isolamento é
alegado. Nenhuma dependência, IPC, acesso remoto ou permissão nova foi criada.
