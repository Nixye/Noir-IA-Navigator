# CSS e layout responsivo próprios

O Noir calcula estes recursos em Rust, sem delegar a um motor Web externo. Este documento descreve um subconjunto implementado, não conformidade geral com CSS. Os testes e as capturas diagnósticas efetivamente executados constam nos relatórios de cada entrega em `docs/validation`.

## Recursos disponíveis

- `display: block`, `inline`, `inline-block`, `flex` e `grid`; `inline-flex` e `inline-grid` são tratados como contêineres de bloco.
- Flex em linha ou coluna, direções inversas, quebra de linhas, espaçamento `gap`/`row-gap`/`column-gap`. Em linha, `flex-grow`, `flex-shrink`, `flex-basis` e o atalho `flex` distribuem o espaço disponível. Larguras de base respeitam dimensões, margens, bordas e padding.
- `justify-content` com início, centro, fim, `space-between`, `space-around` e `space-evenly`. `align-items` com início, centro, fim e extensão visual da caixa externa de altura automática. As áreas de links e controles são deslocadas junto com a pintura.
- Grid com colunas explícitas em `px`, `em`, `rem`, `%`, `fr`, `auto` (equivalente a `1fr`) e `repeat(N, ...)`. Os elementos são colocados em ordem, linha a linha; cada linha usa a altura do conteúdo mais alto. Máximo de 32 colunas.
- `min-width` e `min-height` básicos, além de largura, altura, largura máxima e modelo de caixa já existentes. `inline-block` cria uma caixa própria no fluxo da linha. Alturas percentuais exigem uma altura de referência definida; alturas automáticas não são convertidas artificialmente em zero.
- `position:relative` mantém o espaço no fluxo e desloca pintura e links. `position:absolute` sai do fluxo normal e das distribuições Flex/Grid, usando a padding box do ancestral posicionado mais próximo, ou o viewport inicial. `top/right/bottom/left/inset` aceitam comprimentos e percentuais, com ancoragem à direita/embaixo e largura automática entre left/right. Descendentes absolutos são pintados junto à caixa antes do deslocamento de alinhamento do ancestral.
- `rem` usa o tamanho de fonte computado do elemento html, com 16px iniciais. Unidades vw/vh/vmin/vmax são recalculadas por viewport. Unidades físicas px/pt/pc/in/cm/mm usam 96dpi CSS; não representam medida física calibrada da tela.
- `text-align:left/start/center/right/end`, herança de visibility, nowrap e o atalho font em seu formato básico (peso, tamanho e line-height). Famílias de fontes continuam usando a fonte embutida. O agente de usuário reconhece center, align, nowrap e larguras HTML de tabelas/células.
- Tabelas HTML simples distribuem linhas/células, larguras em px/%/auto, cellspacing/border-spacing e alinhamento do conteúdo, até 32 colunas e 1024 linhas. Sem rowspan/colspan: células com spans usam uma coluna e geram diagnóstico.
- Seletores de atributo `[name]`, `[name=value]`, `~=`, `|=`, `^=`, `$=` e `*=`; valores com aspas podem conter espaços.
- `@media` com `screen`/`all`, condições `min-width`, `max-width`, `min-height`, `max-height`, `width`, `height` e `orientation`. Combinações `and`, alternativas separadas por vírgula e até oito níveis de aninhamento. A cascata é recalculada usando o viewport atual em cada reflow. Características desconhecidas não correspondem.
- O atributo `media` de folhas embutidas e de links externos também é reavaliado no viewport atual. Folhas desabilitadas, tipos diferentes de CSS e links de tema alternativo não são ativados automaticamente. `rel` aceita listas de tokens sem distinguir maiúsculas e minúsculas.

## Limites explícitos

O dimensionamento intrínseco é uma aproximação limitada, com um orçamento de quatro milhões de unidades de trabalho para nós e texto. Não implementa os algoritmos completos `min-content`/`max-content`, congelamento iterativo do Flexbox, alinhamento de baseline, `align-self`, `order`, `wrap-reverse`, crescimento/encolhimento no eixo de colunas ou distribuição por `align-content`. A extensão de caixas não recalcula percentuais de altura dos descendentes.

Grid não possui áreas nomeadas, posicionamento explícito, spans, `minmax`, `auto-fit`/`auto-fill`, tracks de linha nem subgrid. Atalhos `place-*` e escrita vertical não são interpretados. Posicionamento fixed/sticky, floats, tabelas completas, transforms, filtros e animações CSS continuam pendentes. Absolute/relative não implementam z-index/stacking contexts, escrita vertical, todas as regras de static position ou esticamento vertical entre top/bottom. O containing block de elementos inline relativos é uma caixa envolvente aproximada dos fragmentos. Consultas de mídia de preferência, intervalos e `not` não são interpretadas. Pseudoclasses estruturais e lógicas têm o subconjunto descrito abaixo; estados dinâmicos de ponteiro/foco e pseudo-elementos continuam pendentes.

Overflow visível participa da extensão rolável, mesmo quando a altura declarada do pai é menor. Coordenadas e dimensões são limitadas a um milhão de pixels CSS, e geometria não finita produz erro. A 0.8 acrescenta recortes retangulares para overflow:hidden/clip no subconjunto descrito abaixo; clip-path continua ausente. Um clip:rect de área vazia em elemento absoluto oculta sua própria pintura e a visibilidade herdada, cobrindo o padrão comum de texto de acessibilidade fora da tela. Controles com visibility:hidden não são pintados nem expostos à interface e não participam do envio GET baseado na display list.

Folhas externas chegam como assets IPC limitados e são aplicadas na posição original de cada link no DOM, preservando a ordem relativa às folhas embutidas. O href funciona como chave opaca dos bytes autorizados; o CSS não abre caminhos ou URLs. Rel/type/media/disabled/alternate são respeitados. CSS acumulado continua limitado a 256KiB e 1024 regras: uma folha que ultrapassa o limite de bytes é omitida com aviso; limite de regras/aninhamento conserva o prefixo já aceito e interrompe CSS adicional. Orçamentos de correspondência, resolução de variáveis e cópia da cascata continuam produzindo erro quando esgotados.

Seletores não suportados são ignorados com aviso, sem relaxar o orçamento de correspondências. Valores não finitos e dimensões excessivas são rejeitados ou limitados; renderização não recebe acesso direto a arquivos ou rede.

## Exercício manual

Abra `fixtures/responsive-layout.html` no Noir. A página contém navegação flexível, cartões em três colunas e um formulário. Ao reduzir a largura para 760 px ou menos, a grade muda para uma coluna e a navegação usa distribuição em coluna. O formulário usa envio GET normal. Não representa uma alegação de compatibilidade com aplicações modernas arbitrárias.

A fixture `fixtures/positioned-layout.html` exercita dimensões rem, offsets relativos/absolutos, exclusão do fluxo Flex, visibilidade e tabela simples. Esses recursos corrigem defeitos concretos de composição, como a disposição das línguas em torno do globo na inicial da Wikipedia, mas não garantem compatibilidade integral do site.

## Controles e dimensionamento intrínseco

Inputs ocultos têm largura intrínseca zero; botões usam a largura real do rótulo e controles de texto respeitam size/largura CSS. Espaços entre controles vêm de whitespace e margens, sem acréscimo fixo de 8px. Caixas Flex/absolutas de controles não duplicam o fundo e a borda nativos. A display list transporta cor de fundo, cor/espessura de borda, cor do texto e tamanho de fonte para o controle, usados pela interface e pelo rasterizador CPU. O atalho border:none remove a borda; cores de quatro lados usam a primeira cor no subconjunto uniforme atual.

Select simples é um controle próprio: até 256 opções, valor até 1024bytes e rótulo até 256bytes (cortes em fronteiras UTF-8). A seleção usa o primeiro selected ou primeira opção não desabilitada; um valor esvaziado explicitamente pelo DOM não volta automaticamente à primeira opção. Opções de optgroup desabilitado permanecem desabilitadas. Option/optgroup não aparecem como texto comum da página. Múltiplas seleções/listboxes não são implementadas e a interpretação simples é diagnosticada. Opções excedentes são omitidas com diagnóstico. A alteração nativa chega ao DOM por valores tipados e evento change através do supervisor.

A expansão CPU valida aparência, quantidade/opções e textos dos controles antes de copiar a display list, respeita os budgets anteriores e usa o rótulo da opção selecionada. Fontes CPU permanecem limitadas a256px. A interface nativa mantém suas limitações documentadas de fontes e controles.

## Incremento verificado 0.5

O seletor aceita `:not()`, `:is()` e `:where()` com listas de seletores e especificidade correspondente (`:where` não adiciona especificidade), até oito níveis e 16 alternativas por função. A interpretação é conservadora: uma alternativa não suportada torna a função não suportada; não há a recuperação completa de listas perdoáveis do padrão. `:first-child`, `:last-child`, `:only-child`, variantes `of-type`, `:nth-child()`/`nth-last-child()` e variantes `of-type` calculam a posição entre irmãos-elementos com fórmulas inteiras `an+b`, `odd` e `even`. Todos consomem o orçamento compartilhado de correspondência. `:empty`, `:link`/`:any-link`, `:checked` e `:disabled`/`:enabled` básicos também estão presentes; desabilitação herdada por fieldset/legend não é uma implementação completa. Hover, focus, pseudo-elementos e combinadores de irmãos continuam ausentes.

Template e noscript são subárvores inativas no modo com scripting habilitado. Não fornecem estilos, scripts, recursos ou pintura, mesmo com overrides de display; fechamentos de dialog sem open e datalist são ocultos por padrão. Isso impede que folhas do fallback noscript do Google ocultem indevidamente o documento ativo. O HTML de template ainda não usa DocumentFragment completo. A política é compartilhada por DOM, broker, CSS, layout e execução inicial de scripts.

Comentários CSS só são removidos fora de strings, preservando dados citados como URLs. Propriedades não implementadas continuam sujeitas ao orçamento de cópia da cascata, mas não gastam resolução de variáveis/unidades sem que seu resultado possa ser utilizado. O teste de fronteira original voltou a verificar o orçamento correto sem elevar limites nem afrouxar a asserção.

O CPU aceita os mesmos IDs de Data URLs já autorizados pelo protocolo: soma1MiB de IDs de assets e2MiB de referências de imagem. Outros IDs continuam até8192bytes. Os limites de RGBA, pixels e glifos permanecem; erros de glifos e bytes têm diagnósticos distintos antes de copiar/pintar. Os testes relevantes foram executados nesta revisão, em vez de somente compilados como na0.4.

### Índice de candidatos de seletores

O limite de quatro milhões de unidades de correspondência permanece. As regras são indexadas por uma condição necessária do composto mais à direita: ID, primeira classe, tag ou grupo universal. Cada elemento consulta apenas listas candidatas, desduplica por geração e mantém os índices de ordem originais da cascata. Tokenização/hashing e inserções candidatas consomem o mesmo orçamento; classes repetidas não podem expandir a lista em memória. Testes comparam a seleção indexada à varredura completa, exercitam classes repetidas e uma página grande com muitas regras não relacionadas. Comparações de tag/ID rejeitam comprimentos diferentes antes de contabilizar payload que não foi lido, e condições baratas são avaliadas antes de buscas em atributos/classes. Isso evita repetir a leitura de atributos longos para regras impossíveis, sem ampliar limites. Uma navegação real à inicial da Wikipedia, que antes esgotava esse orçamento, voltou a gerar display list e PPM após a alteração.

## Correções de controles e altura — 0.5.1

`min-height` respeita `box-sizing` também em blocos e controles. Em `border-box`, padding e borda são descontados antes de calcular o mínimo de conteúdo, que não fica negativo. Assim, `min-height:40px; padding:10px; border:0; line-height:18px` produz uma caixa de 40px, em vez de somar novamente 20px de padding. Uma altura explícita menor que borda/padding não produz espaço de conteúdo negativo. A referência é [CSS Box Sizing, box-sizing](https://www.w3.org/TR/css-sizing-3/#box-sizing).

Botões Web nativos usam a fonte e o retângulo do motor, sem herdar padding e altura mínima dos botões da shell. Seus rótulos permanecem em uma linha, com truncamento quando a largura declarada é insuficiente. A pintura dos controles é recortada à caixa; o popup de select permanece em camada própria. A fixture `fixtures/form-control-sizing.html` reproduz controles curtos, rótulos longos e mínimos de altura.

Enter em campo simples de texto/senha utiliza o primeiro submit do formulário na ordem da display list, incluindo nome/valor e overrides de ação/método. Um submit padrão desabilitado bloqueia o envio implícito. Sem submit, um formulário com mais de um campo simples não é enviado por Enter. A ordem completa do DOM em layouts que reordenam conteúdo, botões fora da display list, cancelamento por evento submit, validação completa e POST continuam fora desse contrato.

## Seletores adicionais — 0.6

Os combinadores de irmão adjacente (`+`) e irmãos subsequentes (`~`) consideram somente elementos com o mesmo pai; texto e comentários não interrompem a adjacência entre elementos. Cadeias que misturam descendência, filhos e irmãos são avaliadas com busca regressiva limitada, em vez de escolher um ancestral próximo que impeça o restante do seletor de corresponder. Essa busca e cada travessia de irmãos consomem o orçamento de quatro milhões de unidades já existente, sem ampliar o limite. O índice de candidatos continua baseado no composto à direita, com equivalência à avaliação completa testada.

Seletores de atributo aceitam flags `i` e `s`: `i` ignora diferenças entre letras ASCII, preservando distinções Unicode; `s` compara valores exatamente. Os operadores `=`, `~=`, `|=`, `^=`, `$=` e `*=` usam a mesma política. Valores citados podem conter delimitadores como `]`, `+` e `~`; operadores dentro desses valores não são confundidos com a estrutura do seletor. Flags desconhecidas, combinações de flags e escapes CSS ainda não implementados são recusados. A regra de comparação padrão para todos os atributos enumerados do HTML não está completa; use as flags explícitas dentro do subconjunto suportado. Referência: [Selectors Level 4](https://www.w3.org/TR/selectors-4/).

## Identificadores escapados e recorte — 0.8

Identificadores de tag, classe e ID aceitam escapes simples e hexadecimais CSS de um a seis dígitos, com um whitespace CSS opcional após a sequência hexadecimal. Por exemplo, `.mode\:active` corresponde à classe literal `mode:active`, e `#\31 panel` ao ID `1panel`. Separadores escapados não viram combinadores ou vírgulas de lista. CRLF após escape hexadecimal é consumido como uma unidade; escapes inválidos de quebra de linha são recusados. NUL, surrogate e escalares fora do intervalo são substituídos por U+FFFD. A indexação de candidatos usa os identificadores decodificados. Permanecem os limites de 1024 bytes por seletor, 32 compostos, oito níveis e orçamento de correspondência existente.

Escapes em valores/nomes de atributos e nomes de pseudoclasses continuam fora do subconjunto. Essa ampliação não altera querySelector/matches/closest dos bindings, que possuem contrato independente. Seletores não implementados continuam sendo ignorados com aviso; não se removem suas condições para aplicar estilos a elementos incorretos.

`overflow:hidden` e `overflow:clip`, com um único valor, recortam o conteúdo de caixas suportadas à padding box retangular, incluindo recortes aninhados, imagens, texto, controles e links. Ambos usam o mesmo recorte no subconjunto atual; não criam scroll containers CSS. `overflow:visible` restaura a ausência de recorte. A caixa de fundo e borda do próprio elemento permanece fora de seu recorte de conteúdo. Filhos absolutos seguem o containing block limitado já implementado. Inline sem caixa própria, overflow-x/y, dois valores, auto/scroll, border-radius, overflow-clip-margin e clip-path não estão implementados.

O layout emite PushClip/PopClip no IPC v4. O supervisor e os consumidores recusam geometria inválida, pilhas desbalanceadas e mais de 128 níveis. Links são intersectados após todas as translações/alongamentos, antes de expor áreas de interação; o orçamento dessa etapa é de dois milhões de interseções. GPU recorta quads e ajusta UVs; CPU preserva o retângulo original da imagem e a origem da cobertura dos glifos. O desktop usa o mesmo recorte nos controles nativos. O popup de select continua em sua camada nativa própria.

A fixture [clipping-selectors.html](../../fixtures/clipping-selectors.html) acompanha a entrega para avaliação manual. Não foram executados testes, navegação ou validação gráfica dessa ampliação, conforme solicitado pelo usuário.

## Condições e seletores adicionais — 0.9

O parser distingue at-rules encerradas por ponto e vírgula das regras em bloco, respeitando strings, escapes, funções e seletores de atributo. Uma declaração `@import` desconhecida não consome mais a próxima regra CSS. `@charset "UTF-8";` é reconhecida como metadado quando aparece no início da folha já decodificada, opcionalmente após BOM. Isso não muda a decodificação do broker. Importações continuam recusadas: o motor não abre caminhos ou URLs; `@layer`, `@font-face`, animações e outras at-rules desconhecidas continuam diagnosticadas e seus blocos não são achatados na cascata.

Os avisos identificam a at-rule ignorada, por exemplo `@import`, `@layer` ou `@font-face`. Os nomes ficam limitados a 40 caracteres, mensagens repetidas são desduplicadas e novos avisos desse tipo param quando a lista contém 32 diagnósticos; outros limites de diagnósticos existentes permanecem.

`@supports` avalia declarações reconhecidas com valores do subconjunto e `selector(...)` com o parser real de seletores. Aceita negação `not`, conjunções homogêneas `and`, disjunções homogêneas `or` e agrupamento por parênteses. Misturar and/or sem agrupamento não é aceito. Características e funções de consulta desconhecidas são falsas, permitindo fallback por negação. A API Rust `web_css::supports` compartilha esse avaliador; ela não instala por si só o objeto JavaScript CSS.

O reconhecimento é conservador: display block/inline/inline-block/flex/grid/none; position static/relative/absolute; overflow visible/hidden/clip; visibility visible/hidden; cores simples; comprimentos/unidades implementados, margens/padding/inset; peso/tamanho de fonte, line-height, direções e números Flex, alinhamentos, espaçamentos, box-sizing, text-align e white-space reconhecidos. Custom properties de nome limitado e valor balanceado também são reconhecidas. Shorthands complexos, valores com var/calc, templates Grid, fontes externas e capacidades apenas aproximadas como inline-flex/inline-grid não são anunciados por consultas. Um valor não anunciado pode pertencer a outro subconjunto parcialmente implementado; a consulta não promete conformidade geral.

Condições ficam limitadas a 4096 bytes e oito níveis; regras condicionais conservam o máximo de oito níveis já existente. Preludes permitem até 32 delimitadores aninhados, cada bloco limita a varredura a 8192 statements, e o limite agregado de 1024 regras/256 KiB CSS permanece.

Seletores de atributo decodificam escapes CSS simples/hexadecimais em nomes e valores. Strings com aspas aceitam delimitadores escapados e continuação de linha; newline bruto e escape incompleto são recusados. A flag i continua limitada a ASCII. Nomes sem escape conservam o subconjunto alfanumérico anterior; namespaces completos não foram acrescentados. Escapes de nomes de pseudoclasses continuam ausentes.

`:lang(...)` aceita até 16 ranges básicos ASCII de até 128 bytes, como `pt`, `pt-BR` e `en`, inclusive em listas/strings CSS. A comparação é ASCII sem distinção de caixa, com correspondência exata ou prefixo seguido de hífen. O idioma vem do atributo HTML lang mais próximo na cadeia de ancestrais; lang vazio ou inválido interrompe a herança. Ausência de idioma não inventa uma configuração do navegador. Wildcards, filtragem estendida BCP47, XML e `:dir` ainda não estão implementados. Travessias, leitura de atributos e comparações compartilham o orçamento de quatro milhões de unidades.

As regras foram implementadas para avaliação posterior. A fixture [css-conditional-rules.html](../../fixtures/css-conditional-rules.html) apresenta condições verdadeiras/falsas, fallback, idioma herdado, escapes e at-rules não suportadas. Não foram executados testes, navegação ou validação gráfica deste incremento. Referências de sintaxe: [CSS Conditional Rules Level 3](https://www.w3.org/TR/css-conditional-3/#supports-syntax) e [Selectors Level 4](https://www.w3.org/TR/selectors-4/#the-lang-pseudo).

## Especificidade e IDs compostos: primeiro recorte do item 6

A especificidade usa a tupla lexicográfica (IDs, classes/atributos/pseudoclasses, tipos), sem pesos decimais. Cada ocorrência conta, inclusive #a#a e .a.a. :is() e :not() usam a maior tupla de toda a lista, não apenas a alternativa que corresponde; :where() e o seletor universal acrescentam zero. As somas de componentes u32 são verificadas e overflow é recusado, sem saturação ou wraparound.

O matching exige todos os IDs de cada composto: #a#b não corresponde somente a id=b, enquanto #a#a pode corresponder a id=a. O índice usa o primeiro ID como condição necessária do composto à direita e o matcher verifica todos os demais predicados. Permanecem 1024 bytes por seletor, 32 compostos, oito níveis, 16 alternativas por função e quatro milhões de unidades de correspondência; não há recuperação completa de listas lógicas inválidas.

A chave de prioridade é (important, inline, especificidade, ordem da regra/declaração). Assim, !important vence normal, inline vence folha com a mesma importância e a ordem desempata. Não implementa a cascata completa entre origens/camadas. CSSOM, custom properties completas, CSS-wide keywords e conformidade geral continuam pendentes. @media e @supports são recursos anteriores, não novos neste recorte; consultas DOM conservam contrato independente.

Implementação parcial; revisão textual consolidada concluída e checagem estática aprovada, sem validação funcional. A [fixture css-specificity-cascade.html](../../fixtures/css-specificity-cascade.html) contém expectativas visuais, não resultados observados, e não foi executada. Veja [entrega e limites](../releases/2026-11-08-css-specificity.md). O item 6 permanece ABERTO.

## Alinhamento transversal Flex: primeiro recorte do item 7

Flex row/row-reverse nowrap usa a altura de conteúdo definida como tamanho transversal da linha. Percentual exige referência definida; min-height isolado não a define. Min-height maior que height definida só afeta a caixa final, não expande a referência de alinhamento. Center/flex-end consideram margens e permitem offsets negativos para filhos maiores. Stretch só cresce visualmente caixas height:auto, sem encolher, recalcular descendentes percentuais ou esticar controles diretos. Altura automática, wrap, coluna e Grid conservam o caminho anterior.

Pintura, clips próprios, métricas, links e controles seguem o pipeline existente. Consulte o [contrato e números esperados](../releases/2026-11-08-flex-cross-axis.md) e a [fixture não executada](../../fixtures/flex-cross-axis.html), com body margin 0. Implementação parcial; revisão textual final do item 7 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–7 ABERTOS/parciais; item 19 adiado.

## Texto e whitespace: primeiro recorte do item 8

White-space separa preservação de espaços, preservação de quebras e permissão de wrap. Normal/nowrap colapsam whitespace ASCII e quebras; pre-line colapsa espaços/tabs e preserva quebras; pre/pre-wrap preservam espaços e quebras no subconjunto limitado. CRLF/CR são normalizados por nó: CRLF dentro do mesmo nó é uma quebra; CR ao fim de um nó e LF no início do seguinte são duas quebras preservadas, não um par unificado. U+00A0, U+202F e U+2003 não são colapsados como espaços ASCII. Normal, pre-line e pre-wrap permitem wrap; nowrap e pre não. O estilo de agente de usuário de pre passa a impedir wrap; declarações do autor podem sobrescrevê-lo.

Fluxo e largura preferida de cada nó usam classificação linear compartilhada. A largura preferida com quebras é o máximo das larguras das linhas; o agregador de irmãos permanece escalar, sem min-content/max-content completos. Espaço colapsável pendente atravessa nós inline no fluxo, usa a fonte do próximo run e avança sem item, fundo ou link próprios; início/fim de linha e quebras o descartam. A medição raw de controles permanece separada; kerning já existia.

Cada quebra preservada força altura mínima de linha, inclusive linhas vazias; quebra final avança a linha anterior, sem acrescentar uma linha vazia depois dela. Tabs e form feed preservados continuam equivalendo a um espaço cada; espaços preservados não forçam wrap por si. A quebra emergencial ainda é por escalares e pode separar NBSP ou sequências combinantes.

Não acrescenta fontes, fallback, shaping, bidi, graphemes, tab stops completos, seleção, hifenização, justificação ou quebra Unicode completa. Não modifica pintores, IPC ou autoridade de I/O. Veja [contrato e limites](../releases/2026-11-08-text-whitespace.md) e [fixture não executada](../../fixtures/text-whitespace.html). Implementação parcial; revisão textual final do item 8 concluída sem bloqueadores; checagem estática aprovada. Sem validação visual ou funcional. Itens 1–8 ABERTOS/parciais; item 19 adiado.
