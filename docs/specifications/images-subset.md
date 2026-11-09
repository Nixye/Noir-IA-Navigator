# Imagens — subconjunto do Noir 0.4

O broker decodifica bytes já autorizados e entrega RGBA ao renderer. O motor Web
não resolve caminhos ou URLs de imagens por conta própria. O mesmo decoder atende
arquivos confinados, respostas de rede autorizadas e Data URLs de imagem.

## Formatos e limites

| Recurso | Subconjunto |
| --- | --- |
| PNG/JPEG | Decodificação estática pelo crate `image` |
| GIF/WebP | Primeiro frame; animação não é reproduzida |
| SVG | Formas, caminhos, grupos, gradientes e clipping limitado; raster estático |
| Data URL | `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/svg+xml`; base64 ou escapes percentuais |
| Imagem codificada | Até 4 MiB por arquivo/recurso |
| Identificador Data URL | Até 1 MiB, sujeito também ao limite agregado de IDs no documento |
| Fonte raster e canvas SVG | Até 4096 por lado e 4 Mi pixels (4 × 1024 × 1024) |
| Saída | Normalizada preservando proporção até 1024 por lado; até 4 MiB RGBA |
| Documento | O orçamento agregado de RGBA continua 8 MiB |

Imagens maiores que 1024, mas dentro dos limites de origem, são reduzidas em vez
de descartadas. A saída RGBA usa alfa direto; o resultado pré-multiplicado do
rasterizador SVG é convertido antes do IPC. O primeiro frame de GIF/WebP é obtido
sem coletar todos os frames. Cabeçalhos de canvas e frames codificados de
GIF/VP8/VP8L/ANMF são verificados antes de decodificar para impedir que um canvas
pequeno esconda um frame excessivo.

## SVG estático

`resvg`/`usvg` são utilizados somente como codecs de imagem, com recursos padrão
desativados. Não substituem HTML, CSS, layout ou JavaScript do Noir.

A entrada deve ser XML UTF-8 com raiz SVG. DTD e resolutor de entidades externas
não são habilitados. A árvore XML é limitada antes da conversão. Os resolutores
`resolve_string` e `resolve_data` de imagens SVG retornam sempre `None`; o default
que poderia abrir um arquivo nunca é utilizado. Fontes de sistema, texto e SVGZ
não estão habilitados. O parser recebe a árvore previamente verificada, em vez de
reprocessar o XML com opções permissivas.

São aceitos `svg`, `g`, `defs`, `path`, `rect`, `circle`, `ellipse`, `line`,
`polyline`, `polygon`, `linearGradient`, `radialGradient`, `stop`, `clipPath`,
`title` e `desc`. Estilos inline aceitam apenas propriedades simples de pintura,
traço contínuo, opacidade e visibilidade. Somente referências de pintura para
fragmentos do próprio documento são aceitas.

Ficam fora do subconjunto: scripts/handlers, imagens aninhadas, `use`, texto,
folhas `<style>`, fontes, animação, `foreignObject`, filtros, máscaras, padrões,
marcadores, traços pontilhados, hrefs, estilos com escapes/propriedades adicionais,
cadeias e transformações de clipping. Esses SVGs são recusados, sem obter acesso
a qualquer recurso externo. Isto não equivale a suporte completo a SVG.

| Orçamento SVG | Limite |
| --- | --- |
| XML codificado | 256 KiB |
| Nós XML / elementos | 1024 / 512 |
| Profundidade | 16 |
| Dados de caminhos/pontos | 32 KiB |
| Valores numéricos de geometria | 8192; magnitude máxima de um milhão |
| Segmentos convertidos | 8192 |
| Trabalho estimado | Pixels de saída × segmentos até 128 Mi (128 × 1024 × 1024) |
| Camadas intermediárias estimadas | Soma de até 32 MiB |
| Extensões geométricas após escala | 16384 por componente |

O preflight e os limites de trabalho/camadas evitam entradas expansivas antes da
pintura. Não há preempção de uma chamada interna de codec pelo relógio; esses
limites não são uma garantia de tempo de parede em todo hardware. O prazo do
broker não concede recursos ou autoridade adicionais ao decoder.

Uma imagem recusada permanece indisponível; não é substituída por uma renderização
simulada nem por outro navegador. A fixture `fixtures/image-formats.html` permite
avaliação manual do SVG estático e de uma imagem em Data URL. Na entrega anterior de formatos foram
executadas compilação e análise estática; a suíte de testes não foi executada,
conforme pedido do usuário.

Fontes de API: [resvg](https://docs.rs/resvg/latest/resvg/),
[política de resolutores do usvg](https://docs.rs/usvg/latest/usvg/struct.ImageHrefResolver.html)
e [limites de parsing XML](https://docs.rs/roxmltree/latest/roxmltree/struct.ParsingOptions.html).

## Dimensões naturais: item 12 (2026-11-08)

O contrato IPC v6 separa `ImageAsset.width/height: u32`, dimensões físicas da textura RGBA reduzida, de `natural_width/natural_height: f32`, dimensões intrínsecas em CSS px de densidade 1. Os novos campos são obrigatórios, sem default compatível com mensagens v5. Não são getters JavaScript `naturalWidth/naturalHeight` nem seleção de densidade por `srcset`.

Para PNG/JPEG/GIF/WebP, o broker preserva as dimensões de origem obtidas antes do resize. Para SVG, preserva `tree.size()` em ponto flutuante, inclusive frações, antes do arredondamento e da redução da textura. A textura continua normalizada a 1024 por lado e até 4 MiB de RGBA; não se aloca RGBA a partir das dimensões naturais.

O validador comum exige valores finitos e estritamente positivos, até 4096 em cada eixo, com `ceil(natural_width) × ceil(natural_height) <= 4 × 1024 × 1024`. Esses limites de metadados não substituem o comprimento exato `width × height × 4`, os limites físicos e o agregado de 8 MiB de RGBA. O decoder raster mantém o teto de alocação de 32 MiB. Os limites SVG, a política de resolutores sem I/O e o tratamento de formatos animados como primeiro frame permanecem inalterados.

Texturas, uploads GPU e amostragem CPU continuam usando width/height físicos. A mudança de versão descreve uma incompatibilidade de dados, não uma nova autoridade ou fronteira de confiança; não acrescenta acesso a arquivos/rede, dependência ou canal privilegiado.

Estado deste incremento: implementação parcial, revisão estática final concluída sem bloqueadores; checagem estática aprovada. Nenhum teste, fixture, aplicativo ou validação visual/funcional foi executado para este incremento. Evidências da entrega histórica acima e do item 11 não validam o item 12. Veja [release e lacunas](../releases/2026-11-08-natural-image-dimensions.md).

## Dimensionamento do subconjunto

O helper comum de dimensionamento é usado pela largura intrínseca e pela geometria da imagem pintada:

- Com ambos os eixos automáticos, usa as dimensões naturais, inclusive frações; não usa a textura reduzida como tamanho CSS.
- Com um eixo explícito e o outro automático, deriva o automático pela proporção natural. Com ambos explícitos, mantém os dois, sem impor a proporção.
- O valor CSS resolvido tem prioridade sobre o atributo HTML correspondente. Quando não há valor CSS resolvido, aceita atributo numérico finito, positivo e até 16384; auto e percentual não resolvido seguem esse fallback limitado. Não é a cascata completa de presentation hints.
- max-width restringe a largura e recalcula somente a altura automática. Uma altura explícita permanece explícita. No caminho de bloco, max-width já foi aplicado à caixa de conteúdo antes da pintura.
- A largura de conteúdo alocada pelo caminho block/Flex/Grid é usada na pintura; a preferência intrínseca não substitui essa alocação. O bloco de imagem automático parte da largura intrínseca em vez de preencher indiscriminadamente o pai.
- A resolução de altura percentual depende de referência definida. A medição intrínseca sem largura de referência definida não resolve percentuais de largura; não implementa dimensionamento min/max-content completo.

Frações naturais, inclusive abaixo de 1 CSS px, são preservadas em inline, bloco e no avanço de img inline-block; a alocação de Flex/Grid conserva o mínimo legado de 1 CSS px, portanto não há promessa de dimensões subpixel idênticas nesses contêineres. O cálculo mantém limites locais de geometria de 16384 por eixo e usa intermediários f64 para proporções extremas. Imagem ausente/inválida mantém o placeholder existente com proporção sintética 160:48; isso não simula conteúdo carregado. Box sizing, margens, padding, alinhamento e limites dos contêineres seguem o subconjunto do motor, sem algoritmo completo de replaced elements ou relayout geral de Flex/Grid.
