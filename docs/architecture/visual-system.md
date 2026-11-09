# Sistema visual — Noir 0.5

Revisão de 2026-11-02. A página inicial foi recomposta para diferenciar claramente esta versão da 0.3/0.4: cabeçalho assimétrico à esquerda, escultura procedural à direita, favoritos em cartões horizontais com título/domínio e duas ações úteis abaixo. A identidade usa grafite, vidro e acento limão. Não há assets de Death Stranding nem uma captura de interface simulada.

| Token | Cor |
| --- | --- |
| BG | #0e1214 |
| PANEL | #171d1f |
| SURFACE | #21292b |
| ACCENT | #dbefaa |
| TEXT | #eff2eb |
| MUTED | #95a09d |
| LINE | #2f3c3c |

A barra superior mantém grupos e abas. A marca Noir é visível, o endereço fica em uma cápsula com ícone e as ações usam vetores. Não há lateral de abas. A página inicial oferece busca Google, favoritos editáveis, abrir assistente, criar grupo, abrir arquivo e personalizar. Todos os botões correspondem a ações reais.

A escultura de linhas e o brilho são decoração procedural, sem representar progresso, uso de rede, inferência ou telemetria. Hover/transições duram aproximadamente180–250ms; Reduzir animações interrompe movimento decorativo. Alvos de clique permanecem estáveis enquanto superfícies mudam de intensidade.

Os cartões de ações ficam lado a lado em largura suficiente e empilhados em painéis estreitos. Nomes de grupos são limitados/recortados com tooltip; a área rolável inclui o rodapé. A inicial foi capturada no framebuffer nativo em 1280×860 e900×700 com assistente aberto. Capturas do framebuffer não incluem todo o efeito do compositor.

O Windows aplica acrílico à janela transparente; opacidade continua configurável. O desfoque e a tonalidade percebidos variam conforme o sistema e o fundo. Documentos Web mantêm suas cores. Um aviso simples aparece quando existem erros concretos de compatibilidade, com detalhes técnicos opcionais e possibilidade de ocultar o aviso naquele documento.

O ícone original tem monograma N e pontos de extremidade em limão sobre grafite. SVG/PNGs/ICO ficam em apps/browser-desktop/assets; scripts/generate-icon.ps1 reproduz os arquivos. O ícone é usado pela janela e pelos atalhos da instalação atual.

Versão, identificação da compilação e caminho do executável ficam em Configurações → Avançado. A instalação tem caminho estável dist/Noir/noir.exe para impedir confusão entre cópias antigas. A verificação interativa usa --temporary-profile e não grava abas/favoritos da pessoa.
