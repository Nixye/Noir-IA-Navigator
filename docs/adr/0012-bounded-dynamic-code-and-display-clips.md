# ADR 0012 — Código dinâmico limitado e recortes da display list

Status: implementado para validação posterior pelo usuário. Data: 2026-11-07.

## Código dinâmico

`eval` e `Function` usam o parser, AST, escopos e interpretador do Noir. Não há runtime JavaScript externo, compilador nativo, acesso a arquivos, rede ou ferramentas privilegiadas. Eval direto usa o ambiente do chamador; chamadas indiretas e Function usam o ambiente global. Erros de sintaxe ordinários podem ser capturados; esgotamentos de recursos permanecem não capturáveis.

Código dinâmico recebe limite de 64 KiB por entrada, 256 KiB acumulados por VM e oito avaliações aninhadas, além dos limites de instruções, strings, heap e escopos existentes. Falhas de parsing também consomem o orçamento acumulado. Diretivas de modo estrito são recusadas explicitamente, pois sua semântica ainda não foi implementada. O subconjunto e as limitações de completion records constam na especificação JavaScript.

Regex comuns permanecem no mecanismo linear `regex`. Assertions, capturas nomeadas, referências e modificadores aceitos pelo adaptador podem usar a biblioteca de busca `fancy-regex` fixada em 0.19.2. Ela não executa JavaScript. A tradução é limitada, lookbehind exige largura fixa compatível e padrões avançados têm orçamento de backtracking/delegação. Construções não suportadas produzem erro, sem remover condições do padrão para fazê-lo aceitar.

## Fronteira IPC e driver

O protocolo passa de v3 para **v4**, acrescentando `PushClip { rect }` e `PopClip`. Worker e host devem ser recompilados juntos; não há interpretação silenciosa de respostas de protocolo antigo. A página não escolhe comandos de GPU ou shaders: o layout produz dados, e o supervisor valida geometria, balanceamento e profundidade máxima de 128 recortes antes de aceitá-los.

`overflow:hidden` e `overflow:clip` geram recortes retangulares limitados à padding box no subconjunto de caixas suportado. O driver próprio intersecta retângulos e ajusta UVs de imagens/glifos para preservar a amostragem. O rasterizador CPU e os controles nativos respeitam a mesma pilha. Links são recortados no layout para evitar áreas de interação fora da região visível. Os limites de itens, vértices, glifos e trabalho de pintura continuam vigentes.

Este incremento não altera AppContainer, capacidades de sistema, broker de rede, shaders constantes nem permissões de IA. `navigator` e `location` expõem metadados limitados e APIs realmente disponíveis; não acrescentam conectividade ao worker. `sendBeacon` retorna false porque não existe fila de envio. Propriedades não implementadas permanecem ausentes.

## Entrega e evidência

O usuário solicitou implementação sem testes ou validação nesta etapa, substituindo a exigência normal do repositório. Não foram executados testes, Clippy, navegação, avaliação de scripts ou validação gráfica deste incremento. A compilação release é destinada à entrega do executável. Fixtures manuais acompanham o código; não são resultados de execução. A matriz mantém busca Google, aplicação YouTube e reprodução de vídeo sem compatibilidade funcional declarada.
