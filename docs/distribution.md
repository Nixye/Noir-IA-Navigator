# Distribuição atual e identidade do executável

A distribuição instalada usa um único caminho estável: `dist/Noir/noir.exe`.
O ZIP Windows x64 usa `dist/Noir-windows-x64.zip`. Atalhos criados pelo projeto
apontam diretamente ao caminho estável e não a uma pasta com data ou versão.

## Compilar e identificar

```powershell
./scripts/build.ps1 -TargetDirectory target/production
./target/production/release/browser-cli.exe --version
```

O script calcula um SHA-256 dos fontes de aplicações/crates, fixtures,
manifestos, lock e scripts de build/distribuição. Define `NOIR_BUILD_ID` para
Cargo como versão e os primeiros 12 caracteres desse hash. Documentação não
altera essa identidade. Se os fontes mudarem enquanto a compilação ocorre, o
script recusa registrar uma identidade de distribuição.

`noir-build.json`, junto aos executáveis, registra versão, identidade, SHA-256
completo dos fontes, hashes dos dois executáveis, compilador, target e data UTC.
A data varia; a identidade dos mesmos fontes permanece determinística. Builds
diretos por Cargo podem aparecer como `development`; o script deve ser usado
para uma entrega identificada. O executável CLI e o painel Sobre devem permitir
conferir a identidade antes de avaliar mudanças visuais ou de comportamento.

## Preparar a distribuição

```powershell
./scripts/package.ps1 -TargetDirectory target/production -Archive
./scripts/install-current.ps1
```

O empacotador executa os checks normais por padrão. `-SkipTests` é reservado ao
chamador que já executou os checks ou registrou a dispensa. A nova entrega é
montada em uma pasta de estágio, inclui IA local/documentação/licenças e passa
por verificação de hashes antes da promoção ao caminho atual. A versão anterior
fica temporariamente em `.noir-previous-*`; falha de promoção tenta restaurá-la.
Não há encerramento automático de processos: uma distribuição atual em uso
impede sua substituição.

O modelo incluído pode usar hardlink NTFS para os mesmos bytes verificados em
`resources/ai`, economizando cópia de aproximadamente 1,1 GB. Não é junction ou
atalho: remover um nome de arquivo não remove os outros nomes. O ZIP contém os
bytes completos e permanece portátil. `-CopyModel` força cópia independente se
essa característica não for desejada. Não edite os recursos verificados em
nenhum dos caminhos; modificações no conteúdo de um hardlink afetam seus nomes
compartilhados e a verificação do modelo deve recusá-las.

O instalador cria `Noir.lnk` na área de trabalho e no menu Iniciar do usuário.
Ele recusa substituir um atalho de mesmo nome que aponte a outro programa. A
descrição do atalho inclui versão e identidade, e o diretório de trabalho é a
distribuição atual. Perfil e credenciais continuam nos seus locais próprios.

## Limpar artefatos antigos

```powershell
./scripts/cleanup-old-distributions.ps1 -IncludeStaleBuildBinaries
./scripts/cleanup-old-distributions.ps1 -IncludeStaleBuildBinaries -Apply
```

Sem `-Apply`, o comando só mostra a lista. A aplicação da limpeza exige uma
distribuição atual com hashes verificados. São elegíveis apenas pastas e ZIPs
legados reconhecidos diretamente em `dist`, backups de distribuições atuais e,
com a opção indicada, executáveis obsoletos conhecidos de `target/debug` e
`target/release`. Executáveis que já correspondem à distribuição atual não são
considerados obsoletos.

Caminhos finais precisam permanecer no diretório pretendido; junctions,
symlinks, caminhos fora da raiz e artefatos em execução são recusados antes da
remoção. A limpeza não remove `dist/Noir`, o ZIP estável, fontes, recursos
canônicos de IA, perfil ou cache de dependências do Cargo. Tamanhos exibidos são
lógicos e não garantem o espaço físico recuperado quando há hardlinks.

Atalhos antigos de mensagens do chat podem deixar de abrir depois que suas
versões forem removidas. Use o atalho instalado ou o caminho estável atual.
