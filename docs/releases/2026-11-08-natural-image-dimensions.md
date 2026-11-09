# Item 12: dimensões naturais de imagens

Estado: implementação parcial; revisão estática final do item 12 concluída sem bloqueadores; checagem estática aprovada. Itens 1-12 ABERTOS/parciais; item 19 adiado. Sem execução de testes, fixtures, aplicativo ou validação visual/funcional. O executável em dist/Noir não foi reconstruído; não há publicação ou nova distribuição.

## Contrato de dados

- ImageAsset distingue width/height u32 da textura RGBA e natural_width/natural_height f32 obrigatórios, em CSS px de densidade 1. Raster conserva as dimensões anteriores ao resize; SVG conserva tree.size() fracionário antes do arredondamento da textura.
- O IPC passa de v5 para v6 pela mudança incompatível do esquema. Não há default para os novos campos. Ambos os participantes devem usar a mesma versão; não há fallback silencioso para mensagens antigas.
- Naturais devem ser finitas, positivas, até 4096 por lado e ceil(width) × ceil(height) até 4 Mi pixels. São metadados de layout, nunca dimensões de alocação RGBA. Bytes, textura, upload GPU e sampling CPU permanecem vinculados às dimensões físicas.
- Permanecem: imagem codificada até 4 MiB, fonte até 4 Mi pixels, alocação do decoder raster até 32 MiB, textura até 1024 por lado/4 MiB RGBA e agregado de 8 MiB RGBA. Mi significa múltiplos de 1024.
- SVG mantém XML até 256 KiB, 1024 nós/512 elementos, profundidade 16, 8192 segmentos, trabalho estimado até 128 Mi e camadas até 32 MiB, além dos limites anteriores de geometria. Resolutores continuam sem I/O; o broker é o único carregador autorizado.

A versão registra mudança de dados, sem nova autoridade, acesso a arquivos/rede, dependência ou canal privilegiado. Não há alteração de fronteira de confiança que exija um novo ADR. Veja o [contrato de imagens](../specifications/images-subset.md).

## Dimensionamento do subconjunto

O helper comum de dimensionamento é usado pela largura intrínseca e pela geometria da imagem pintada:

- Com ambos os eixos automáticos, usa as dimensões naturais, inclusive frações; não usa a textura reduzida como tamanho CSS.
- Com um eixo explícito e o outro automático, deriva o automático pela proporção natural. Com ambos explícitos, mantém os dois, sem impor a proporção.
- O valor CSS resolvido tem prioridade sobre o atributo HTML correspondente. Quando não há valor CSS resolvido, aceita atributo numérico finito, positivo e até 16384; auto e percentual não resolvido seguem esse fallback limitado. Não é a cascata completa de presentation hints.
- max-width restringe a largura e recalcula somente a altura automática. Uma altura explícita permanece explícita. No caminho de bloco, max-width já foi aplicado à caixa de conteúdo antes da pintura.
- A largura de conteúdo alocada pelo caminho block/Flex/Grid é usada na pintura; a preferência intrínseca não substitui essa alocação. O bloco de imagem automático parte da largura intrínseca em vez de preencher indiscriminadamente o pai.
- A resolução de altura percentual depende de referência definida. A medição intrínseca sem largura de referência definida não resolve percentuais de largura; não implementa dimensionamento min/max-content completo.

Frações naturais, inclusive abaixo de 1 CSS px, são preservadas em inline, bloco e no avanço de img inline-block; a alocação de Flex/Grid conserva o mínimo legado de 1 CSS px, portanto não há promessa de dimensões subpixel idênticas nesses contêineres. O cálculo mantém limites locais de geometria de 16384 por eixo e usa intermediários f64 para proporções extremas. Imagem ausente/inválida mantém o placeholder existente com proporção sintética 160:48; isso não simula conteúdo carregado. Box sizing, margens, padding, alinhamento e limites dos contêineres seguem o subconjunto do motor, sem algoritmo completo de replaced elements ou relayout geral de Flex/Grid.
## Fixture e expectativas

A [fixture natural-image-dimensions.html](../../fixtures/natural-image-dimensions.html) contém somente HTML/CSS e SVGs em Data URLs. Não foi executada. Documenta origem 2048 × 1024 reduzida a textura 1024 × 512, origem fracionária 100,5 × 50,25, altura explícita 40 derivando largura 80, dois eixos 90 × 30, CSS prevalecendo sobre atributo, max-width 60 derivando altura 30 e caminhos block/Flex/Grid. São expectativas de contrato, não medições ou validação visual.

## Lacunas e evidências

Este recorte não implementa replaced elements completos, algoritmo geral de min/max, object-fit, srcset/sizes/picture, carregamento progressivo, animação, SVG DOM, Canvas 2D, OffscreenCanvas, ImageBitmap, WebGL ou WebGPU. Também não adiciona getters JavaScript naturalWidth/naturalHeight. O item 12 permanece ABERTO/parcial.

A aprovação estática do item 11 (check 34936, exit 0 em 3,03 s, somente warning preexistente Stmt::Function) pertence ao item anterior e não valida este incremento. Revisão estática final específica do item 12 concluída sem bloqueadores, após ajuste do avanço fracionário de img inline-block; checagem estática aprovada na integração. Código-fonte e expectativas documentadas não são resultados de execução.

## Checagem estática registrada

Na integração do item 12, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check da sessão 85125 terminou com exit 0 em 1,00 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído. A revisão estática final foi concluída sem bloqueadores após corrigir o avanço fracionário de img inline-block, que também preserva dimensões abaixo de 1 CSS px. Flex/Grid conservam o mínimo legado de alocação de 1 CSS px.

Nenhum teste, fixture ou aplicativo foi executado; não houve validação funcional ou visual. As regressões-fonte e a fixture continuam preparadas para execução futura. Checagem estática não comprova comportamento funcional, conformidade Web ou compatibilidade geral. O item 12 permanece ABERTO/parcial e o item 19 continua adiado.
