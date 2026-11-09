# Item 6: especificidade CSS e IDs compostos

Estado: implementação parcial; revisão textual consolidada concluída e checagem estática aprovada. Sem validação funcional. Itens 1–6 ABERTOS/parciais; item 19 adiado. Não foram executados testes, fixtures ou aplicativo neste recorte. O executável em dist/Noir não foi reconstruído; não há publicação ou nova distribuição.

## Incremento e contrato

- Especificidade comparada lexicograficamente como (IDs, classes/atributos/pseudoclasses, tipos), sem pesos decimais. Componentes u32 usam soma verificada: overflow é recusado, sem saturação ou wraparound. Um ID supera qualquer quantidade aceita de classes; uma classe supera qualquer quantidade aceita de tipos.
- Cada ocorrência conta: #a#a tem (2,0,0), enquanto .a.a tem (0,2,0). O seletor universal não acrescenta especificidade.
- :is() e :not() usam a maior tupla de toda a lista de argumentos, mesmo quando a alternativa de maior especificidade não corresponde ao elemento. :where() acrescenta zero, inclusive para argumentos com IDs. Conserva-se o subconjunto de listas e aninhamento existente.
- Todos os IDs do mesmo composto são exigidos. #a#b não corresponde a um elemento somente com id=b; #a#a pode corresponder a id=a, contando duas ocorrências na cascata.
- A prioridade inline é uma dimensão separada, não um número artificial de pontos. Dentro do subconjunto atual, !important vence normal; inline vence folha com a mesma importância; a ordem da regra/declaração desempata prioridades iguais.
- O índice usa o primeiro ID quando disponível como condição necessária do composto à direita; ser candidato não elimina a verificação de todos os IDs e demais condições. O orçamento compartilhado de matching permanece.

## Limites e fronteiras

Mantêm-se 1024 bytes por seletor, 32 compostos, oito níveis de funções lógicas e 16 alternativas por função; CSS acumulado até 256 KiB e 1024 regras. Correspondência permanece limitada a quatro milhões de unidades. Listas lógicas não implementam recuperação completa de alternativas inválidas. Limites de cópia da cascata e resolução de variáveis permanecem vigentes.

Nenhuma dependência, API JavaScript, I/O ou permissão nova. O motor recebe bytes autorizados pelo broker. Este recorte não altera o contrato independente de querySelector/matches/closest. @media e @supports já existiam e não são novidades desta entrega.

CSSOM, folhas dinâmicas completas, custom properties completas, CSS-wide keywords, camadas, origens completas da cascata, escopos e conformidade geral permanecem abertos. O item 6 não está concluído.

## Fonte e evidências

A [fixture css-specificity-cascade.html](../../fixtures/css-specificity-cascade.html) reúne expectativas visuais, sem JavaScript nem recursos externos: categorias lexicográficas, repetições, IDs contraditórios, listas lógicas, inline, !important e empate por ordem. Verde é o resultado esperado, não um resultado observado. A fixture não foi executada.

Revisão textual consolidada concluída sem bloqueadores; checagem estática aprovada; evidências dos itens anteriores não validam este incremento. A compilação estática registrada não é validação funcional nem comprovação de compatibilidade CSS geral.

Referências: [Selectors Level 4, especificidade](https://www.w3.org/TR/selectors-4/#specificity-rules) e [CSS Cascading Level 5](https://www.w3.org/TR/css-cascade-5/).

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check terminou com exit 0 em 1,69 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído. Foram acrescentadas seis regressões em código-fonte e ampliada a cobertura-fonte do índice; nenhuma foi executada.

Checagem estática aprovada; revisão textual consolidada concluída sem bloqueadores, incluindo as seis regressões-fonte e a cobertura ampliada do índice. Não houve execução de testes, fixtures, aplicativo ou Clippy. Compilação estática e regressões-fonte não comprovam comportamento funcional. O item 6 permanece ABERTO/parcial.
