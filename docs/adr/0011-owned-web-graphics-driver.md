# ADR 0011 — Driver de pintura Web próprio

Status: implementado, aguardando validação do usuário. Data: 2026-11-07.

## Decisão

`web-graphics` transforma a display list em uma cena imutável: geometria de triângulos, comandos ordenados, texturas autorizadas e um atlas de cobertura de glifos. A fonte embutida é rasterizada por fontdue; a construção de geometria, empacotamento do atlas, limites e preparação da cena são código do Noir. Não há caminhos, URLs ou dispositivos abertos a partir do conteúdo nesse crate.

O backend `page_gpu` da aplicação executa um shader WGSL próprio sobre wgpu e o dispositivo da janela. Retângulos, texto e imagens da página não são delegados às primitivas de pintura do egui nesse caminho. egui mantém a janela, o render pass e os controles de entrada nativos. Lotes contíguos com a mesma textura são unidos sem alterar a ordem; fronteiras de controles e destaques preservam a composição.

Esse é um driver de renderização da aplicação em espaço de usuário. O driver de hardware continua fornecido pelo sistema operacional; não há instalação de driver de kernel. O rasterizador CPU próprio existente em `web-paint` permanece usado pelo CLI. Uma falha de recursos da cena GPU é reportada, sem rotulá-la como fallback CPU de desktop completo.

## Fronteiras e limites

O worker continua recebendo bytes do broker e devolvendo dados IPC validados. Somente shaders constantes do executável são usados; a página não fornece WGSL. A cena é preparada a partir dos dados recebidos e confere dimensões, texto, IDs, contagens e bytes antes de copiar texturas. O adaptador confere limites do dispositivo e intervalos de vértices antes de upload. O protocolo IPC e a política AppContainer permanecem iguais.

Limites: 40 mil itens, 100 mil glifos, 4096 combinações distintas de glifo/tamanho/peso, atlas R8 de 2048×2048, 16 milhões de pixels de trabalho de raster de glifos, 900 mil vértices, 34 texturas e 16 MiB de bytes de textura por cena. Imagens continuam limitadas pelo broker a 8 MiB agregados e 1024 por lado. O driver cobra até 256 milhões de pixels de trabalho visível por quadro, contando cada quad, inclusive glifos coincidentes, antes de emitir comandos.

Há uma cena CPU ativa e cache de duas cenas GPU. Scroll preserva a geometria; zoom/DPI e uma nova geração do documento substituem a cena. A textura de glifos usa padding transparente, e imagens são carregadas com alpha premultiplicado. A filtragem de imagens permanece bilinear de oito bits em espaço gamma, sem gerenciamento de cor completo. Negrito usa expansão sintética da cobertura; shaping complexo e famílias de fonte arbitrárias continuam ausentes.

## Entrega

A solicitação atual adia testes e validação para o usuário. Esta alteração não recebeu testes automáticos, Clippy, navegações ou validação de shader/driver. A compilação dos executáveis serve à entrega. `fixtures/graphics-driver.html` oferece conteúdo para avaliação manual de texto, alpha, imagens, controles, zoom e atualização de documento.

## Complemento de 2026-11-08: item 16, rasterização após clip

Este complemento preserva o registro histórico de 2026-11-07 acima. O primeiro recorte do item 16 separa métricas de glifos da materialização no atlas. A interseção usa os limites de tinta com ascent, kerning, escala e negrito sintético antes de produzir bitmap. Glifos totalmente recortados não acrescentam entradas ao atlas nem quads, mas preservam avanço e caractere anterior. Glifos parcialmente visíveis mantêm o recorte de geometria/UV existente.

A validação global de entrada, incluindo 100 mil glifos e limites de bytes, continua anterior ao descarte. As 4096 combinações distintas do atlas e o orçamento de rasterização cobram materialização; não se acrescenta cache ilimitado de métricas. A cena retida não descarta texto pelo viewport atual: texto abaixo da janela permanece disponível para scroll, e o backend continua recortando a apresentação.

Este complemento não otimiza texturas de imagens nem acrescenta paginação do atlas, tiles, shaping, transforms completos, composição, canvas/vídeo, recuperação de device loss ou fallback CPU desktop. Checagem estática do item 16 aprovada; revisão estática final concluída sem bloqueadores; sem testes, validação visual/GPU ou desempenho medido. Item 16 ABERTO/parcial, como os itens 1-15; item 19 adiado. A [nota de entrega](../releases/2026-11-08-clipped-glyph-raster.md) detalha o alcance atual.

O buffer fixo de cobertura do atlas (2048 × 2048 bytes) continua alocado na construção da cena. Evitar entradas de glifos recortados não elimina essa alocação nem significa consumo zero de memória.

Após calcular e validar os limites de tinta, o caminho também evita materializar glifos com área zero ou alpha igual a zero, preservando avanço e kerning. Isso não dispensa a validação da entrada nem constitui medição de desempenho.
