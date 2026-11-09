# Item 16: glifos recortados antes da rasterização

Data: 2026-11-08. Estado: implementação parcial. Checagem estática aprovada; revisão estática final concluída sem bloqueadores. Itens 1-16 ABERTOS/parciais; item 19 adiado.

## Recorte implementado

O construtor de cena de `web-graphics` separa `GlyphMetrics`/`Atlas::metrics` de `Atlas::glyph`, que materializa bitmap e entrada no atlas. As métricas dos glifos já materializados são reutilizadas; glifos ocultos não alimentam outro cache. Usa o retângulo real de tinta, considerando ascent, kerning, escala e expansão do negrito sintético, para decidir a interseção com o clip efetivo antes da rasterização.

Glifos totalmente fora desse clip não são rasterizados, não acrescentam entradas ao atlas e não emitem quads. Mesmo quando descartados, mantêm o avanço da caneta e o caractere anterior usado no kerning. Glifos parcialmente visíveis continuam materializados e têm geometria/UV recortados pelo caminho existente. A decisão é por glifo, não por um retângulo aproximado do texto inteiro.

O clip efetivo de conteúdo não é o viewport atual. A cena retida preserva texto abaixo ou fora do viewport para a rolagem posterior; o backend continua fazendo seu recorte por viewport na apresentação. Não foi introduzido descarte por viewport na construção da cena.

## Limites e fronteiras preservados

- A validação global da entrada continua antes do descarte: texto oculto não contorna validação de geometrias, texto, contagens ou bytes.
- O limite global de 100 mil glifos da entrada permanece, inclusive para conteúdo recortado. Os limites existentes de bytes continuam intactos.
- O limite de 4096 combinações glifo/tamanho/peso do atlas e o orçamento de 16 milhões de pixels de rasterização incidem sobre glifos efetivamente materializados. Não se introduz um novo cache de métricas sem limite.
- O atlas continua R8 de 2048 × 2048, sem paginação. Demais limites de cena, texturas, vértices e trabalho visível do backend continuam aplicáveis.
- IPC, broker, sandbox, shader e backend GPU não recebem novas permissões. O crate continua sem abrir caminhos ou URLs do conteúdo.

## Lacunas mantidas

Este recorte não otimiza a preparação/upload das texturas de imagens, não implementa paginação do atlas, tiles, shaping, transforms completos, árvores de propriedades, composição de camadas/superfícies, canvas/vídeo, recuperação de device loss ou fallback CPU completo no desktop. O rasterizador CPU do CLI não constitui esse fallback. O item 16 permanece aberto.

## Verificação e entrega

Checagem estática do item 16 aprovada; revisão estática final concluída sem bloqueadores. Não foram executados testes, fixtures, aplicativo, navegação ou validação GPU nesta etapa de documentação. Não se declara equivalência visual, compatibilidade funcional ou ganho de desempenho medido. A redução de trabalho descrita é o comportamento do caminho de construção, não um benchmark.

O executável em `dist/Noir` não foi reconstruído nem publicado por este recorte. A checagem aprovada do item 15 não valida o item 16. Oito regressões em código-fonte acompanham a implementação, mas não foram executadas e não constituem evidência de aprovação funcional.

Veja o [complemento ao ADR 0011](../adr/0011-owned-web-graphics-driver.md), a [matriz de capacidades](../architecture/capabilities.md) e o [roadmap detalhado](../roadmap/implementações%20Planejadas.md).

O buffer fixo de cobertura do atlas (2048 × 2048 bytes) continua alocado na construção da cena. Evitar entradas de glifos recortados não elimina essa alocação nem significa consumo zero de memória.

Após calcular e validar os limites de tinta, o caminho também evita materializar glifos com área zero ou alpha igual a zero, preservando avanço e kerning. Isso não dispensa a validação da entrada nem constitui medição de desempenho.

Checagem estática registrada: sessão 57813, saída 0, 1,02 s, com cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked. Apenas o warning preexistente de Stmt::Function. Nenhum teste, render, aplicativo ou execução GPU foi realizado; a revisão estática final foi concluída sem bloqueadores.

A revisão estática não substitui execução nem demonstra cobertura completa. Não há regressões explícitas em código-fonte para alpha zero, espaço, acento ou escala fracionária nesta entrega; esses cenários permanecem sem essa cobertura específica.
