# classList: continuação nova da base V13

Estado registrado em 2026-11-12, na conclusão da autoria, antes da validação:
NOT_RUN. Resultados posteriores pertencem ao relatório externo do checkpoint,
não a este registro de autoria. Este bloco parte da base V13 restaurada;
não é recuperação nem validação das versões V14–49.

## Escopo

Element.classList expõe um handle DOMTokenList retido com identidade estável e
leituras vivas do atributo class. O subconjunto inclui length, índices, item,
value/stringifier, contains, add/remove variádicos, toggle com force e replace.
Tokens são separados por whitespace ASCII; a lista elimina duplicatas mantendo
a ordem. Getter value e stringifier conservam o atributo bruto; operações de
atualização serializam tokens conforme seu contrato, sem normalização precoce.

A ponte Runtime converte cada argumento relevante uma vez, em ordem, antes da
validação dos tokens e da mutação. Hooks podem editar class; a operação lê o
estado corrente depois deles. Token inválido em add/remove/toggle/replace não
publica parte da operação. Efeitos explícitos anteriores de hooks e falhas
Resource seguem as regras transacionais existentes, não uma promessa de
rollback de toda exceção ordinária.

contains com token vazio ou contendo whitespace retorna false. item exige um
argumento; item(undefined) converte para índice zero. toggle(token, undefined)
usa force omitido. replace preserva a primeira posição entre o token antigo e
o novo, sem duplicação. Não expõe iteradores, forEach, construtor/protótipos ou
DOMTokenList Web completo.

## Conversão da própria lista

String(classList) e a conversão primitiva da lista usam uma ponte estreita
Host::stringifier, opt-in, cujo padrão é None. DomHost habilita essa ponte
somente para handles CLASS; não chama propriedades ou métodos arbitrários de
outros objetos host. O resultado é o atributo class bruto e vivo. A VM cobra
o trabalho e a string retornada sob seus limites existentes; a leitura DOM
também mantém seu orçamento. Essa ponte não generaliza coerção de todos os
objetos DOM nem remove limites de tamanho, alocação ou execução.

## Limites locais

A implementação é limitada: até 32768 bytes de texto/resultado e argumentos
convertidos acumulados, até 4096 tokens e 256 argumentos variádicos, além dos
orçamentos DOM/VM existentes. Essas cotas são escolhas locais, não limites
normativos da Web. Leituras vivas e reserva transacional não autorizam acesso
a arquivos/rede nem removem limites de recursos.

## Integração escrita

A fixture classlist-recovery-r1.html usa menu, aba ativa e classe de tema. Uma
lista retida altera classes no script inicial, num requestAnimationFrame e num
click encaminhado pelas APIs existentes. Promise e contadores permitem detectar
replay e consumo duplicado de callbacks. O cenário é inline com origem noir:;
a origem interna não recebe autoridade de carregamento remoto ou file.

Os testes renderer observam atributo corrente, identidade do handle, seletores
e frames reais de CSS/layout: cor, largura e texto da aba. Os testes worker
observam o mesmo processo, reflow, jobs/RAF, click e recuperação de erro de token
sem publicação parcial. Não há mudanças de produção renderer/CLI, IPC,
dependências, permissões ou segurança de rede neste recorte de integração.

Na conclusão da autoria em 2026-11-12, antes da validação, esses testes estavam
escritos e ainda não executados. O relatório externo do checkpoint registrará
os resultados posteriores e a identidade das fontes efetivamente testadas.
Este documento não alega validação de GUI, Windows, sites públicos, Google,
WPT ou isolamento do sistema.

Referências normativas: https://dom.spec.whatwg.org/#interface-domtokenlist,
https://infra.spec.whatwg.org/#set-replace e https://webidl.spec.whatwg.org/.

## Limitação preexistente de pintura observada no diagnóstico local

Em 2026-11-12, após a primeira tentativa de testes, duas renderizações locais
com binário existente mostraram fundo da caixa e fundo do fragmento textual
na mesma cor: caixa 120×24 e texto Menu 40,816×24; no controle estático sem
JavaScript/classList, caixa 40×24 e texto Box 27,376003×24. O segundo retângulo
coincide com a geometria do texto, não representa outra caixa de layout.
As fontes CSS/layout desse caminho são idênticas à base V13 preservada.

Os oráculos de integração distinguem a caixa pela cor e dimensões completas,
exigem exatamente uma caixa esperada e recusam qualquer fundo da cor oposta.
Não pressupõem quantidade total de primitivas por cor. A sobrepintura do fundo
do texto é uma limitação preexistente separada, não corrigida por classList.
Não foi testada composição alpha/transparência; esse diagnóstico opaco não
comprova sua correção. Resultados da nova tentativa pertencem ao relatório
externo do checkpoint, não são antecipados por esta correção de oráculo.
