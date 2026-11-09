# ADR 0006 — Ferramentas de navegação propostas pela IA

Data: 2026-11-02. Status: implementado; validação interativa pendente.

## Fronteira de confiança

O serviço de inferência não recebe handles do navegador. Ele retorna um objeto JSON com uma única proposta pertencente a um vocabulário fechado: ler página, navegar, abrir ou alternar abas, selecionar um elemento, digitar em um campo e clicar. Não há ferramenta de execução de código, acesso a arquivos, terminal ou acesso direto à rede. A interface nativa valida, autoriza e despacha a proposta pelos mesmos caminhos utilizados pela pessoa.

O modo de navegação precisa ser ativado no painel. Há permissões separadas para leitura, navegação, abas, seleção, digitação e cliques; apenas leitura vem marcada dentro desse modo. Uma ferramenta sem permissão solicita autorização para aquela proposta. No modo granular, cliques exibem revisão mesmo com a permissão habilitada, porque podem enviar formulários ou confirmar operações. A recusa encerra a sequência. As permissões são da conversa e não são persistidas no perfil.

A observação vem da página já confirmada pela interface e contém a identidade da aba e a geração do documento. Ela possui texto visível, metadados, lista limitada de abas e identificadores dos elementos expostos pelo shell. Senhas e campos ocultos não são observáveis nem editáveis por essas ferramentas. A resposta do modelo fica vinculada à observação original: mudar a aba ou a geração invalida a proposta. Navegações e eventos só produzem sucesso depois de seu resultado real ser confirmado.

Dados da página entram em mensagem de usuário delimitada como observação JSON não confiável, nunca no papel de sistema. Rótulos, títulos, URLs e resultados de ferramentas não podem conceder permissões. Essa separação e a validação estrutural reduzem a autoridade do conteúdo; não constituem prova de imunidade a prompt injection. A interface, e nunca os dados da página, decide a autorização: no modo granular há revisão de cliques; a opção de permissão total concede antecipadamente todas as ferramentas de navegador implementadas.

## Limites e compatibilidade

No modo granular, no máximo oito ações são encadeadas por pedido. Em Permissão total, o limite é 32 ações por pedido, sem interrupções para autorização. Há uma proposta por resposta e limites de texto, tamanho de observação e quantidade de elementos. O modelo recebe apenas texto e controles expostos, sem percepção visual de imagens, canvas ou vídeo. Selecionar refere-se aos elementos expostos; não há seleção arbitrária por coordenadas ou execução de seletores da página.

O runtime usa `chat/completions` com `response_format: json_object`. Provedores externos precisam aceitar esse formato. Uma resposta fora do esquema causa erro visível, sem execução parcial. O modelo local de 1.5B pode ter dificuldade em planejar ou respeitar o formato; a implementação não garante sucesso de tarefas arbitrárias. O contexto local passa a 4096 tokens para acomodar o protocolo e as observações.

Este ADR complementa o ADR 0003 e registra a evolução da interface antes apenas conversacional. IA incluída, configuração de provedor externo explícito e inicialização em segundo plano já haviam substituído as premissas iniciais daquele documento.

Não foram executados testes nesta alteração, conforme solicitação expressa do usuário. Compilação e verificações estáticas não substituem avaliação funcional ou de segurança das ferramentas.

## Despacho na interface

O dispatcher aceita apenas HTTP/HTTPS sem credenciais na URL (além da inicial interna), valida IDs contra a observação atual e recusa arquivos, senhas e campos ocultos. No modo granular, formulários com controles de senha exigem envio manual; em Permissão total, formulários GET suportados podem ser enviados com os valores já preenchidos, sem expor os valores dos campos de senha na observação enviada ao modelo. POST continua fora do subconjunto. O shell espera o commit real de navegação/evento antes de continuar, com prazo de 90 s e descarte em mudanças independentes da aba ou geração. Timers da página ativa são suspensos durante planejamento/revisão para evitar que a observação seja invalidada indefinidamente. Seleção foca campos e destaca/rola até elementos expostos; não permite coordenadas arbitrárias.

## Permissão total opcional

A pessoa pode ativar **Permissão total** diretamente no painel do Assistente de navegação. Essa escolha concede leitura, navegação, abas, seleção, digitação e cliques de uma vez, sem solicitar aprovações por ação ou revisão de cliques. O runtime recebe todas as ferramentas como autorizadas e a UI despacha as propostas válidas automaticamente. O modo fica identificado como ativo e oferece **Revogar permissão total** e **Interromper**. Não há diálogo de confirmação para ativá-lo.

A concessão vale somente para a conversa e não é gravada em disco. Nova conversa, desativação do Assistente de navegação e troca/reinício de configuração do modelo revogam o modo total. Alterar permissões interrompe inferências e propostas ainda não despachadas; uma operação que já começou pode terminar, sem iniciar outra em seguida. O limite de 32 ações por pedido evita um loop ilimitado e pode ser retomado por uma nova mensagem.

Permissão total autoriza o vocabulário já implementado do navegador. Ela não cria ferramentas de terminal/arquivos, não libera leitura de senhas/campos ocultos, não altera o transporte ou a política do broker e não ignora IDs, esquema JSON ou geração do documento. Páginas continuam sendo dados não confiáveis; nenhuma página pode ativar o modo ou conceder uma permissão. No modo granular anterior, as regras de aprovação continuam disponíveis.

## Complemento de 2026-11-08: observação limitada desde a extração

O primeiro recorte do item 17 mantém a fronteira deste ADR e o vocabulário de ações. O snapshot textual deriva de texto/display list, controles e links, não de uma API integral de DOM/layout/acessibilidade/imagem. A extração passa a copiar incrementalmente sob limites locais; a preparação para o modelo usa dados emprestados, aplica permissões antes de copiar/serializar e limita bytes UTF-8 durante a serialização, sem serializar tudo para truncar depois. Falha devolve erro estruturado e não envia JSON parcial.

Sem leitura autorizada, campos de página e elementos são removidos; sem permissão de abas, sua lista é removida. Isso não altera full_access, concessões, escopo aba/geração, ferramentas ou orçamento de ações. A geração já muda quando items/links/title mudam. Timers e descarte de propostas antigas são preservados. Cancelamento não reverte uma operação já iniciada.

A exclusão de password/hidden não significa observação livre de credenciais: texto visível, URLs e valores de campos comuns podem conter segredos. Página continua sendo dado não confiável, com validação estrutural e autorização nativa, não imunidade a prompt injection. O provedor configurado continua recebendo o contexto autorizado.

[Contrato, limites e estado](../releases/2026-11-08-bounded-ai-observation.md). Item 17 parcial; checagem estática aprovada; revisão estática final concluída sem bloqueadores, sem testes, inferência ou validação funcional neste recorte.
