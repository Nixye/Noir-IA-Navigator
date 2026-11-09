# Item 8: texto e whitespace normalizado por nó

Estado: Implementação parcial; revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–8 ABERTOS/parciais; item 19 adiado. Não foram executados testes, fixtures ou aplicativo nesta documentação. O executável em dist/Noir não foi reconstruído; não há publicação ou nova distribuição.

## Contrato do recorte

- White-space distingue preservar espaços, preservar quebras e permitir wrap. Normal/nowrap colapsam whitespace ASCII e quebras em um espaço; pre-line colapsa espaços/tabs, mas preserva quebras. Pre/pre-wrap preservam espaços e quebras em subconjunto limitado. Normal/pre-line/pre-wrap permitem wrap; nowrap/pre não. Pre de agente de usuário passa a impedir wrap; o autor pode sobrescrever o estilo.
- CRLF dentro do mesmo nó de texto é uma quebra; CR isolado é normalizado para quebra. A normalização reinicia em cada nó: CR no final de um nó e LF no início do próximo produzem duas quebras preservadas, não um par CRLF unificado. U+00A0 (NBSP), U+202F e U+2003 são conteúdo preservado, não separadores ASCII. A quebra emergencial ainda é por escalares e pode romper NBSP ou sequências combinantes; não há quebra Unicode/graphemes completos.
- Cada LF/CRLF/CR preservado força altura mínima de linha, inclusive linhas vazias. Uma quebra final avança a linha anterior, mas não inventa outra linha vazia após ela. Tabs e form feed preservados continuam equivalendo a um espaço cada; espaços preservados não forçam wrap por si. Isso não implementa tab stops ou pre-wrap completos.
- Classificação linear compartilhada entre fluxo e largura preferida por nó evita medir texto bruto quando o fluxo o colapsa. Com quebras preservadas, a largura preferida é o máximo das linhas, não a soma. O agregador de irmãos continua escalar e aproximado, sem min-content/max-content completos nem equivalência universal entre texto único e fragmentado.
- Espaço colapsável pendente atravessa nós inline no fluxo, usa a fonte do próximo run e avança sem item de pintura, fundo ou área de link próprios. Início/fim de linha e quebras o descartam. Espaço descartado não gera item nem área clicável.
- measure_text permanece raw para controles. O kerning preexistente é preservado; não é uma capacidade nova deste recorte. Não há alteração de fontes ou pintores.

## Fixture e expectativas

A [fixture text-whitespace.html](../../fixtures/text-whitespace.html) é HTML/CSS local sem scripts ou recursos externos. Não foi executada. As expectativas descrevem o contrato, não resultados observados ou medições de pixels.

- A mesma entrada com espaços repetidos, tab e quebra aparece nos cinco modos. Normal/nowrap colapsam separadores; pre-line conserva quebras sem conservar espaços repetidos; pre/pre-wrap conservam espaços no subconjunto descrito.
- Caixas estreitas distinguem wrap de nowrap/pre; não se exige número fixo de linhas dependente da fonte.
- Nós inline contíguos com espaços nas bordas conservam apenas um separador necessário entre palavras. Links somente de espaços nas bordas não produzem área clicável para espaços descartados; links usam fragmentos locais.
- Referências numéricas com CRLF/CR/LF devem produzir três quebras entre A/B/C/D em pre-line. A, duas quebras, B e quebra final ocupa três linhas de conteúdo/linha vazia, sem uma quarta linha vazia final, nos modos que preservam quebras.
- U+00A0/U+202F/U+2003 não viram espaços ASCII nem desaparecem. Preservação não promete glifos disponíveis ou fallback.
- Caixas inline-block com um nó comparam texto colapsado com entrada já normalizada e múltiplas linhas com a maior linha. Não comprovam dimensionamento intrínseco completo entre irmãos.

## Limites e lacunas

Permanecem quatro milhões de unidades de trabalho de layout, 40.000 itens, extensão de um milhão de pixels CSS e largura intrínseca limitada a 16.384 px. Os scans/cópias de NodeText são prepagos: clone cobra len e fluxo cobra 3 × len; a medição intrínseca cobra 2 × len, usando o comprimento em bytes da entrada. A multiplicação saturante e a subtração verificada debitam antes do trabalho. O orçamento existente é compartilhado; estes tetos não prometem processamento simultâneo de todos os máximos.

Não inclui novas fontes, fallback, shaping, bidi, graphemes, tab stops completos, seleção, hifenização, justificação, quebra Unicode completa ou algoritmos intrínsecos completos. Pre/pre-wrap permanecem limitados. Não altera IPC, broker, pintores ou permissões de rede/arquivo.

Revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Aprovações e resultados anteriores não validam este recorte. Não há validação visual/funcional, conformidade CSS ou conclusão do item 8 declarada.

Referência: [CSS Text: white-space](https://www.w3.org/TR/css-text-3/#white-space-property).

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check terminou com exit 0 em 1,69 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído. O recorte contém sete regressões-fonte (uma CSS e seis de layout), nenhuma executada.

Checagem estática aprovada; revisão textual final concluída sem bloqueadores. Não houve execução de testes, Clippy, fixtures ou aplicativo. Compilação estática e regressões-fonte não comprovam comportamento funcional ou visual. O item 8 permanece ABERTO/parcial.
