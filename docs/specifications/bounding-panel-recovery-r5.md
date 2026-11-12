# getBoundingClientRect: recovery-r5

Registro de autoria em 2026-11-12, antes da validação. Resultados posteriores
constarão no relatório externo do checkpoint, ligados às fontes testadas.
R5 continua a V13 restaurada via R1–R4, não recupera V14–49.

## Geometria e retorno

Element.getBoundingClientRect() retorna um objeto ordinário novo com oito
números: x, y, left, top, right, bottom, width e height. É snapshot mutável pelo
chamador, não objeto vivo: alterá-lo não muda o DOM nem a próxima medição.
Não implementa DOMRect completo, protótipos, iteradores ou fragmentos inline.

O produtor entrega border-box principal real de bloco, flex, grid, inline-block,
controles e imagens suportados. Inclui borda/padding, não margem, preserva
frações e posições negativas. Clipping não reduz o retângulo e visibility:hidden
conserva geometria. Ausência de caixa (display:none, detached, conteúdo inativo,
input hidden) retorna oito zeros, inclusive sem contexto de viewport conhecido.

Inline não substituído sem medição, estruturas de tabela não suportadas, imagens
inline decoradas e descendentes atômicos sem geometria retornam erro explícito.
Não se fabrica retângulo de texto/fundo para contornar essas exclusões nem se
confunde NoBox com geometria não implementada.

## Contexto do host e cache

Viewport transporta view_context opcional: Origin explícito, Sampled com
scroll_x/scroll_y/clip_width/clip_height, ou None desconhecido. Default desconhecido
não significa zero. Para caixa existente, origem desconhecida recusa a leitura,
em vez de devolver coordenadas do documento fingindo coordenadas de viewport.

O cache guarda coordenadas e extents de documento; o getter subtrai scroll
limitado pelos extents atuais e clip CSS real. Alterar apenas scroll não renova
nem consome outra resolução de layout. Mutações de DOM/CSS e mudanças relevantes
de viewport mantêm a invalidação e os orçamentos existentes. Não há resolução
ilimitada nem dispensa de limites VM/DOM por medir repetidamente.

IPC10 transporta contexto explícito. O host novo de Begin/headless declara Origin
explicitamente; o worker preserva None como Unknown, sem fallback. Scripts,
Event, Tick e RAF instalam contexto antes dos callbacks. Contexto inválido não
pode deixar origem anterior utilizável. Dados continuam do host, não concedem
I/O, navegação ou autoridade de rede à página. Não são prova nova de sandbox.

## Integração escrita

Fixture bounding-panel-recovery-r5.html mede primeiro getBoundingClientRect,
antes de outra capacidade inédita. Popup é ancorado por diferenças entre caixas
da âncora e painel relativo. Bordas/padding fracionários produzem 90×34; classe
wide altera largura para 110; o snapshot original permanece 90. Promise, RAF e
click permitem conferir retenção sem replay e geometria concordante com o frame.

Testes renderer escritos verificam posição/frações e frame, objeto antigo
estático/mutável, mais de 64 atualizações só de scroll, unknown recusado,
visibility:hidden/clipping sem redução, inline não medido recusado e NoBox zero.
Worker retido verifica PID/reflow/RAF/click. Teste IPC fornece bytes de script
inline autorizados diretamente numa continuação pendente e confere contexto
Sampled antes de Scripts, depois None antes de RAF, sem buscar later.js.

Entradas usam noir: e recursos inline. Nenhum diagnóstico A/B ou teste foi
executado nesta autoria. Não se alega GUI/Windows/sites públicos/Google/WPT,
DOMRect/CSSOM View completo, transformações ou enforcement de isolamento.
