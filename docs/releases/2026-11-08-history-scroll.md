# Item 10: restauração de rolagem no histórico nativo

Estado: implementação parcial; revisão textual final do item 10 concluída sem bloqueadores; checagem estática aprovada. Sem validação funcional ou visual. Itens 1–10 ABERTOS/parciais; item 19 adiado. Testes, fixtures e aplicativo não foram executados nesta documentação. O executável em dist/Noir não foi reconstruído; não há nova distribuição, commit ou publicação.

## Contrato do recorte

- Cada entrada do histórico nativo conserva URL e posição bidimensional de rolagem em pixels CSS. O limite é de 200 entradas por aba. Entradas diferentes da mesma URL não compartilham uma posição global.
- A captura usa o offset efetivamente retornado por ScrollArea::show, dividido pelo zoom da página. Não aplica um segundo fator de DPI. As coordenadas armazenadas são finitas e não negativas, com teto de 1.000.000 pixels CSS por eixo.
- Push descarta o ramo adiante e começa no topo. Cria uma entrada quando a URL difere da entrada atual; para a mesma URL atual, não duplica a entrada, mas zera sua posição. Refresh reinicia no topo sem criar uma entrada adicional. Essas mudanças dependem do commit válido da navegação.
- Traverse, pelos controles nativos de voltar/avançar, solicita a posição da entrada de destino somente após aceitar o resultado da navegação. A restauração é aplicada uma vez, ao apresentar a página correta, não a cada quadro.
- A posição é limitada ao conteúdo e viewport exibidos. Se a página recarregada ficou menor, restaura-se apenas a posição possível; o offset efetivo passa a ser a posição registrada. Zoom e redimensionamento não prometem preservar um elemento semântico ou a mesma composição visual.
- Respostas de Reflow, Event, Input, Change e Tick retêm o documento e não reiniciam a rolagem como uma nova navegação. Falhas e respostas obsoletas não comprometem uma entrada nova nem armam restauração de outro destino.
- O estado pertence à aba, separado das demais abas e workspaces. Uma restauração pendente não é consumida ao exibir outra aba/página. HOME e SETTINGS são superfícies nativas e não fornecem captura de rolagem de documento.

## Fronteiras e lacunas

O estado existe somente na memória da sessão do aplicativo. Não é serializado no perfil e não retorna após reiniciar o Noir. A restauração opcional de endereços/abas do perfil não passa a restaurar esse histórico ou seus offsets.

Voltar/avançar continua recarregando documento e VM. Não há BFCache, preservação de formulários, heap JavaScript, DOM mutado ou estado geral de página. Não implementa History API JavaScript, history.scrollRestoration, navegação por fragmentos, Location mutável, iframes, popups ou novos contextos. Não altera a política de navegação transacional nem concede novos canais IPC, I/O, permissões ou acesso direto do motor a arquivos/rede.

O recorte trata da área de rolagem de documento na shell. Não acrescenta scroll containers CSS, ancoragem de rolagem, scroll snap ou conformidade completa com o histórico de sessão Web.

## Fixtures-fonte para navegação futura

[Documento A](../../fixtures/history-scroll/a.html) e [documento B](../../fixtures/history-scroll/b.html) contêm blocos altos e largos, marcadores identificáveis e links locais A↔B em diferentes alturas. São fontes estáticas sem scripts, imagens, downloads ou dependências remotas. Não foram abertas ou executadas.

Roteiro para uma futura avaliação manual, não executada nesta etapa:

1. Abrir A, deslocar nos dois eixos e seguir um link para B. B deve começar no topo. Deslocar B e usar Voltar: a posição possível de A deve ser restaurada. Avançar deve recuperar a de B.
2. Voltar, rolar A novamente e navegar para B por link: a nova entrada deve começar no topo, sem reutilizar o offset da antiga entrada B. Recarregar deve começar no topo, sem criar nova entrada.
3. Repetir com zoom e viewport diferentes, inclusive quando a posição anterior não cabe no conteúdo. Espera-se clamp geométrico, não restauração do mesmo elemento.
4. Alternar abas/workspaces, inclusive antes da apresentação do resultado. Cada documento deve manter seu próprio estado, sem consumir uma restauração pendente alheia. Visitar HOME/SETTINGS não deve registrar seus offsets como documento.
5. Observar futuras respostas retidas e falhas/navegações sobrepostas: não devem resetar o documento aceito nem aplicar posição de resposta obsoleta. As duas fixtures estáticas, isoladamente, não demonstram todos esses cenários.
6. Reiniciar o aplicativo: não se espera recuperação de offsets ou histórico por este recorte, mesmo quando a preferência de restauração de abas está ativa.

Todos os resultados acima são expectativas do contrato de fonte, não observações de execução. A revisão textual final foi concluída sem bloqueadores; a checagem estática está registrada abaixo. Resultados de itens anteriores não validam o item 10.

## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. A sessão 57949 terminou com exit 0 em 1,03 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído.

Há oito regressões-fonte, incluindo a identidade real/reset de ScrollArea e clamp. Não foram executadas. A revisão textual de código terminou sem bloqueadores após corrigir o IdSalt e a aplicação de limites antes do primeiro quadro, com repaint. A revisão final da documentação foi concluída sem bloqueadores, incluindo as precisões sobre Push para a mesma URL e o teto de coordenadas.

Não foram executados testes, egui, Noir, aplicativo, fixtures, Clippy ou validação visual/funcional. A checagem estática não constitui build de distribuição, resultado funcional ou conformidade Web. O item 10 permanece ABERTO/parcial; o item 19 continua adiado.
