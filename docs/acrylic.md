# Aparência, acrílico e fixação

O fundo da janela pode usar o efeito acrílico nativo do compositor do Windows.
A preferência `acrylic_enabled` permite desligá-lo; `acrylic_opacity` guarda a
opacidade da tonalidade entre 30 e 255 (255 é opaco). O efeito de desfoque é
fornecido pelo Windows, por meio da biblioteca de janela
[window-vibrancy](https://github.com/tauri-apps/window-vibrancy). Não há motor Web
externo nessa dependência. A janela precisa ser criada como transparente e a
interface precisa desenhar superfícies com alfa para revelar o efeito.

O sistema pode limitar efeitos de transparência conforme sua configuração,
composição ou sessão remota. Uma falha na aplicação do efeito produz um aviso;
não é apresentada como sucesso. Plataformas diferentes do Windows conservam a
interface, mas não oferecem o efeito acrílico nesta implementação. A biblioteca
registra limitações de desempenho durante movimento e redimensionamento em
algumas versões do Windows.

O perfil v2 mantém compatibilidade com perfis existentes. Os novos campos têm
valores padrão: acrílico ativo, opacidade 185, dados técnicos ocultos, nenhum
grupo ou aba fixado. Opacidade é limitada ao intervalo permitido ao carregar.

Cada espaço guarda `pinned` para fixação do grupo e `pinned_tabs` como posições
das suas abas. Ao carregar, endereços inválidos são removidos e as posições das
abas fixadas e da aba ativa são remapeadas para a lista validada. Índices
repetidos, fora do limite e referentes a abas removidas são descartados.

Fixar uma aba é uma escolha explícita de salvar seu endereço. Portanto, abas
fixadas são retidas mesmo com a restauração geral da sessão desativada. Nesse
modo, abas não fixadas são removidas do perfil salvo e da cópia de segurança.
Desfixar uma aba remove essa retenção na próxima gravação quando a restauração
geral está desativada. Nenhum conteúdo de página ou credencial é salvo no perfil.

Nesta alteração não foram executados testes automatizados, conforme solicitado.
