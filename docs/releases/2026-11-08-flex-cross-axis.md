# Item 7: alinhamento transversal de Flex em linha

Estado: implementação parcial; revisão textual final do item 7 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–7 ABERTOS/parciais; item 19 adiado. Não foram executados testes, fixtures ou aplicativo. O executável em dist/Noir não foi reconstruído; não há publicação ou nova distribuição.

## Incremento e contrato

- Em Flex row/row-reverse com nowrap, a altura de conteúdo definida determina o tamanho transversal da linha, em vez de somente a maior caixa externa dos filhos.
- Altura definida inclui comprimentos resolvidos e percentual com referência definida. Padding e borda são descontados conforme box-sizing. Min-height isolado não cria uma referência definida para percentuais. Limitação: min-height maior que height definida só é aplicado ao tamanho final da caixa, sem expandir a referência de alinhamento.
- Align-items center e flex-end alinham a caixa externa, incluindo margens superior/inferior. Para linha L e altura externa H, o deslocamento é (L-H)/2 ou L-H. Pode ser negativo quando H excede L; a margem superior separa o início externo da border box do filho.
- Stretch apenas cresce visualmente caixas height:auto pelo helper existente. Não encolhe caixas maiores, não recalcula alturas percentuais de descendentes e não estica controles diretos. Alturas explícitas não são esticadas. Não há relayout completo.
- Sem altura definida, com wrap, em coluna ou em Grid, permanece o caminho anterior. Não muda distribuição no eixo principal, align-content ou align-self.
- Pintura, clips próprios, métricas, links e controles usam o pipeline existente de deslocamento/extensão; clips ancestrais continuam fixos e links passam pela interseção final. Não há API JavaScript, cache, canal IPC, dependência ou autoridade de arquivo/rede nova.

## Fixture e números esperados

A [fixture flex-cross-axis.html](../../fixtures/flex-cross-axis.html) contém HTML/CSS local, declara body margin:0 e não foi executada. Estes números são expectativas de contrato, não medições observadas. Coordenadas y são relativas ao início da área de conteúdo de cada contêiner, não à página.

- Linha de 100 px, filho com border box de 20 px e margens superior/inferior de 10/6 px: altura externa 36 px, y da border box 42 px no centro e 74 px no fim. Row-reverse conserva esses valores transversais.
- Filho de 140 px com as mesmas margens: altura externa 156 px, y=-18 px no centro e -46 px no fim. Overflow hidden recorta a pintura excedente sem substituir o deslocamento negativo por zero.
- Filho automático menor, com as mesmas margens: border box estendida visualmente a 84 px, y=10 px. O filho de altura explícita de 20 px permanece com 20 px.
- Height:120px com box-sizing:border-box, padding:8px e border:2px deixa 100 px de conteúdo; origem de conteúdo 10 px após o topo externo, filho centralizado a 52 px do topo externo.
- Flex height:50% dentro de pai com conteúdo de 200 px resolve 100 px. Pai somente com min-height:200px não torna esse percentual definido.

A fixture inclui links e controle dentro de caixa deslocada e recortada para inspeção futura de pintura/hit testing. Não verifica automaticamente interação, métricas ou rasterização. Regressões em código-fonte não equivalem a regressões executadas.

## Lacunas e evidências

Continuam abertos Flexbox completo, baseline, align-self, order, wrap-reverse, align-content, crescimento/encolhimento em coluna, dimensionamento intrínseco completo, Grid completo, floats, fragmentação, fixed/sticky, stacking contexts, scroll containers e geometria Web completa. O item 7 não está concluído e não há alegação de conformidade CSS.

Revisão textual final específica concluída sem bloqueadores; checagem estática aprovada na integração. Resultados dos itens anteriores, inclusive revisão/check do item 6, não validam o item 7. Testes, Clippy, execução da fixture e validação visual/funcional permanecem adiados.

Referência: [CSS Flexbox, tamanho e alinhamento transversal](https://www.w3.org/TR/css-flexbox-1/#cross-sizing).

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check terminou com exit 0 em 1,01 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído. Foram acrescentadas sete regressões-fonte de layout e uma do renderer; nenhuma foi executada.

Checagem estática aprovada; revisão textual final concluída sem bloqueadores. Não houve execução de testes, fixtures, aplicativo ou Clippy. Compilação estática e regressões-fonte não comprovam comportamento funcional ou visual. O item 7 permanece ABERTO/parcial.
