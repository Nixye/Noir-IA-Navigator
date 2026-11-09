# Espaços de trabalho, favoritos e perfil v2

Cada grupo (espaço) possui um identificador estável, nome, cor, estado de fixação, lista de endereços, posições de abas fixadas e índice da aba ativa. A organização fica no topo; não há lateral de abas. Trocar de espaço seleciona seu próprio conjunto de abas. A interface mantém os objetos das abas enquanto a aplicação está aberta; o perfil não armazena DOM, conteúdo de página, histórico completo de navegação, contexto de IA ou credenciais.

O perfil começa com os espaços **Pessoal** e **Trabalho**. A aplicação pode criar, renomear, selecionar e remover espaços, preservando pelo menos um espaço. Identificadores removidos não são reutilizados pela alocação normal do perfil.

Os favoritos começam com destinos reais para Google, Wikipedia, GitHub e a página interna de recursos do Noir. Isso não representa uma promessa de compatibilidade completa com cada site. Cada favorito possui identificador, título, destino e cor. Inclusões duplicadas retornam o favorito existente; a edição não pode criar dois favoritos com o mesmo endereço normalizado. Esquemas ativos como `javascript:` e `data:`, URLs com credenciais e arquivos hospedados em compartilhamentos de rede não são salvos como favoritos.

## Persistência e privacidade

A restauração de abas continua sendo uma preferência explícita, desativada por padrão. Com ela desativada, os nomes e cores dos espaços, favoritos, espaço selecionado e preferências continuam salvos, e apenas as abas explicitamente fixadas conservam endereços. Abas não fixadas são descartadas em todos os grupos; um grupo sem abas fixadas começa na página inicial. Desativar a restauração remove endereços não fixados da cópia de recuperação. Desfixar também elimina do backup destinos anteriormente fixados.

O formato v1 é migrado em memória para o formato v2. As abas legadas passam para o espaço Pessoal somente quando a restauração já estava habilitada. A gravação usa arquivos temporários exclusivos no mesmo diretório, sincroniza seus dados e substitui o destino atomicamente. Uma cópia válida anterior fica em `profile.json.bak`; JSON truncado ou inválido pode ser recuperado dessa cópia. A API `load_with_report` informa à interface quando a recuperação ocorreu. Uma versão futura desconhecida retorna erro e é preservada, inclusive quando a aplicação tenta salvar preferências.

Falhas de gravação são retornadas ao chamador. O perfil não é uma sincronização entre várias instâncias: cada instância deve ser dona do seu estado ou coordenar alterações antes de salvar.

## Limites

| Dado | Limite |
| --- | --- |
| Espaços de trabalho | 8 |
| Abas por espaço | 24 |
| Abas no perfil inteiro | 128 |
| Favoritos | 64 |
| Nome de espaço | 40 caracteres |
| Título de favorito | 80 caracteres |
| Endereço | 4096 bytes |
| Arquivo de perfil | 1 MiB |

Os limites de abas também precisam ser aplicados pela interface durante a execução; a normalização do perfil não substitui os limites de processos e memória da aplicação. Endereços salvos nunca concedem acesso direto ao renderer: o broker continua responsável por autorizar e carregar cada navegação.

## Validação

Os testes de `browser-config` cobrem independência de abas entre espaços, seleção, nomes e identificadores, migração v1, restauração opcional, remoção de endereços de sessão no arquivo principal e backup, recuperação de JSON inválido, preservação de versões futuras, favoritos editáveis/deduplicados e limites de estado excessivo ou corrompido.

## Interface 0.3

Grupos são selecionados no menu superior; grupos fixados recebem atalhos (até dois adicionais visíveis, os demais permanecem no menu). Abas fixadas aparecem primeiro, compactas. O menu da aba oferece fixar/desfixar, mover, criar grupo com a aba e fechar. Fixar não impede fechar explicitamente. Seleção e sessões preservam identidade durante movimentação. Configurações usam a aba interna `noir:settings`; acrílico, opacidade e movimento ficam no perfil.

Os testes históricos descritos acima não foram reexecutados para este incremento, conforme solicitação do usuário. A compilação dos alvos continua sendo verificada.
