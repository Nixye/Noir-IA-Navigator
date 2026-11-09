# ADR 0008 — Recursos públicos autorizados e folhas de estilo em ordem

Data: 2026-11-02. Estado: implementado; testes automatizados não executados nesta entrega a pedido do usuário.

## Problema

A política anterior aceitava apenas subrecursos de mesma origem. Páginas que entregam CSS, imagens ou scripts clássicos por CDN ficavam sem esses recursos. O broker também inseria todo CSS externo no fim do cabeçalho, mudando sua posição relativa ao CSS inline e quebrando a cascata.

## Decisão e fronteira de confiança

O broker agora pode buscar CSS, imagens e scripts clássicos em outras origens públicas. Cada destino e cada redirecionamento preservam validação de esquema, portas 80/443, recusa de credenciais na URL, DNS público fixado, recusa de redes privadas/reservadas e prazo global de 25 segundos. Uma página HTTPS não pode carregar recursos HTTP. O transporte não herda proxies, não encaminha cookies/chaves de serviços de IA e não envia Referer. Recursos entre origens são dados não confiáveis e não concedem acesso ao serviço de inferência, filesystem ou rede ao renderer.

Isso amplia a autorização de leitura pública do broker. Não implementa CORS, Fetch, cookies ou CSP completos. Não altera o requisito de AppContainer verificado para toda navegação remota Windows; não há fallback sem isolamento e remoto Linux continua desativado.

O protocolo IPC passa à versão 3. DocumentInput carrega StylesheetAsset {href,source}. O href é somente chave opaca de correspondência com o link no DOM. O CSS é aplicado no local original de cada link, junto às folhas inline. Bytes CSS nunca voltam ao parser de HTML: strings como </style> não podem fabricar elementos nem scripts.

Arquivos locais CSS/JS só podem ser lidos mediante o diretório autorizado pelo HTML local. O broker exige caminho relativo, extensão correspondente, canonicalização, permanência sob a raiz e tamanho limitado. Nenhum crate web-* abre arquivos ou URLs.

## Orçamentos

- HTML 1 MiB, IPC 48 MiB e RGBA total8 MiB permanecem.
- Até8 folhas externas;128 KiB por folha;256 KiB total. O CSS mantém orçamento combinado de 256 KiB com folhas inline,1024 regras e budgets de trabalho/cascata. Excesso deixa diagnóstico e conserva prefixo limitado quando aplicável.
- Scripts: até 4 tentativas,128 KiB por fonte e256 KiB total. Não se implementam módulos ou carregamento dinâmico nesta alteração.
- Imagens remotas: até 12 tentativas,4 MiB codificados por imagem, saída até 1024×1024. PNG/JPEG/WebP/GIF (primeiro frame), Data URLs e SVG estático seguem o subconjunto e limites em images-subset.md.
- IDs de Data URLs podem ter até 1 MiB, com soma1 MiB nos assets e2 MiB de referências na displaylist; outros IDs continuam até 8192 bytes. RGBA e frames IPC mantêm os limites anteriores.

SVG é decodificado no broker como imagem estática, com uma biblioteca específica de imagem. Resolutores de arquivos, URLs e imagens embutidas estão desabilitados; sem fontes, scripts, animação ou DOM Web. XML não permite DTD; a validação limita nós, profundidade, geometria, camadas e trabalho. Não é inclusão de um motor Web ou JavaScript de outro navegador. Há orçamento de trabalho, sem preempção por relógio durante o decoder.

## Evidência e limites

Foram adicionadas verificações de fronteira em testes de código para recursos públicos/mixed content, duplicação e tamanho de folhas IPC. Os alvos são compilados, mas esses testes não foram executados nesta entrega. Compilação/Clippy e navegações diagnósticas reais são registrados separadamente em docs/validation. Não há alegação de conformidade geral ou auditoria independente.
