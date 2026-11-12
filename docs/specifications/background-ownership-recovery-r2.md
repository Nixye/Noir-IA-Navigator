# Proprietário do fundo: recovery-r2

Estado registrado em 2026-11-12, na conclusão da autoria, antes da validação:
testes escritos, ainda não executados. Resultados posteriores e identidade das
fontes testadas pertencem ao relatório externo do checkpoint. R2 é continuação
nova da V13 restaurada via R1, não recuperação das versões V14–49.

## Defeito e recorte

No diagnóstico local R1, um bloco de fundo translúcido podia produzir tanto o
fundo da caixa quanto outro fundo sob seu texto direto. CSS computado do nó de
texto retinha o fundo do pai e fragment() o pintava novamente. Com opacidade
parcial, essa sobreposição altera pixels sob o texto mesmo se os glifos são
transparentes. Não se trata de deduplicar cores: caixas e camadas inline
legítimas podem ter a mesma cor e devem compor separadamente.

R2 acompanha o proprietário real da caixa durante block(). Somente um Text
cujo pai já pinta essa caixa recebe background transparente na cópia temporária
do estilo de pintura. O estilo CSS computado permanece intacto. O proprietário
anterior é restaurado ao sair, inclusive quando o layout retorna erro.

O caminho abrange bloco, inline-block, flex/grid, itens blockificados e caixas
posicionadas que entram em block(). Fundos próprios de spans/links inline
permanecem. Não implementa camadas completas de ancestrais inline, preenchimento
de gaps inline ou regras de propagação html/body para o canvas.

## Integração e oráculos

Fixture nova background-ownership-recovery-r2.html: canvas branco, caixas
não-root, glifos transparentes, fundo vermelho rgba(...,0.5) e troca retida
por classList/RAF para azul rgba(...,0.25). Outra caixa mantém camada inline
verde legítima. Um filho absoluto é recortado pelo overflow:hidden do pai.

O parser quantiza alpha por truncamento: .5 → 127; .25 → 63. O rasterizador
CPU existente calcula cada canal por divisão inteira de
(source × alpha + destination × (255 − alpha)) / 255. Sobre branco, uma
composição vermelha dá [255,128,128]; azul dá [192,192,255]. Verde alpha127
sobre vermelho dá [128,191,64]. Vermelho alpha127 sobre outro vermelho igual
dá [255,64,64], exigido somente para duas camadas legítimas distintas.

Renderer verifica cor, quantidade de caixas proprietárias, dimensões, geometria
da camada inline e posicionamento/clip. Worker retido verifica PID, Promise,
RAF e ausência de replay. Surface.rgb é comparado numa região sob o antigo
fragmento textual e noutra região da mesma caixa, além da camada inline e fora
do clip. Caso separado mantém duas camadas vermelhas idênticas, para impedir
uma falsa correção baseada em deduplicação por cor.

Nenhum teste histórico R1 é relaxado. Não há mudança de produção em renderer,
CLI ou web-paint, dependências, IPC, rede ou política de segurança. A origem
noir: identifica bytes sintéticos inline, sem autoridade de acesso remoto/file.

## Limites de evidência

Na conclusão desta autoria, nenhum teste/formatter/Cargo/produto R2 foi
executado. Não há alegação de GUI, Windows, GPU, sites públicos, Google, WPT,
composição Web completa ou enforcement de isolamento validado. A composição
CPU escrita é um oráculo local para o recorte, não equivalência visual geral.
