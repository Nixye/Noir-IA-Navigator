# Captura e replay diagnóstico de DocumentInput

O CLI oferece uma captura explícita da entrada autorizada pelo broker, antes da
execução de scripts. Ela complementa `render --output`, que salva somente o
`RenderedPage`, sem as fontes necessárias para reproduzir um problema.

## Uso

```powershell
browser-cli capture .\pagina.html --snapshot .\caso.noir-snapshot --width 1024 --height 768
browser-cli replay .\caso.noir-snapshot --output .\replay.json --ppm .\replay.ppm
```

`capture` também aceita os alvos normais do broker, inclusive URL HTTP/HTTPS e
páginas internas. A captura de uma página remota usa as mesmas verificações,
cotas, restrições de redirecionamento e autorização de recursos da navegação
normal. Não há caminho alternativo de download, descoberta adicional de recursos
nem execução de scripts durante a captura. Um destino já existente é recusado
antes de carregar o alvo, e a criação usa `create_new` para impedir sobrescrita
mesmo sob corrida. Uma falha de escrita pode deixar um arquivo parcial, que o
leitor recusará. Escolha outro nome ou remova conscientemente o arquivo parcial.

`replay` não chama o broker. Ele entrega o documento preservado ao caminho normal
`browser_process::render_in_subprocess`, sem abrir URL ou identificador de recurso
contido no documento. URLs HTTP/HTTPS permanecem HTTP/HTTPS e exigem o AppContainer
verificado no Windows; não há conversão para `file:`/`noir:` ou fallback sem
sandbox. Outras plataformas recusam replay remoto quando não há backend de
isolamento. A verificação padrão da sandbox pode fazer seus testes negativos de
acesso a arquivo/rede/processo; isso não é uma busca de recurso da página.

O viewport vem do arquivo e não pode ser substituído no replay. Os arquivos
`--output` e `--ppm` do replay também exigem destinos novos. O comportamento de
sobrescrita já existente de `render` não foi alterado.

Não há captura automática, hook de navegação, exportação de aba, perfil, cookie,
credencial ou storage real. `render` não aceita `--snapshot`. O formato fixa o
storage como **Unavailable, com zero entradas**, exatamente como o renderer
descartável do CLI. Não simula storage `ReadWrite` vazio e não reproduz uma sessão
logada do desktop.

## O que é preservado

- URL final de `DocumentInput`, sem normalizar ou reescrever seus bytes.
- HTML, fontes de scripts clássicos/módulos e CSS exatamente como entregues pelo
  broker. Inclui identificadores `src`, `href` e `noir-module:` sem reinterpretá-los.
- Imagens RGBA autorizadas, dimensões físicas e dimensões naturais; metadados de
  áudio do documento local, sem amostras de áudio.
- Viewport e identidade de versão do formato, IPC, pacote CLI e `NOIR_BUILD_ID`.

São os bytes de **DocumentInput**, não uma captura de rede: o broker já pode ter
decodificado charset, removido BOM, descomprimido respostas, reduzido imagens ou
recusado recursos antes da captura. Respostas HTTP brutas, cabeçalhos, corpos de
recursos recusados, fontes de fontes tipográficas externas e estados pós-execução
não são acrescentados ao arquivo.

O replay fixa as fontes de entrada, mas não promete resultados binários idênticos:
PID, tempos, memória, relógio, aleatoriedade e plataforma podem variar. Ele faz
somente a renderização descartável normal, sem reproduzir eventos, frames ou
avanços de timer do desktop. Isso permite investigar uma primeira exceção sem
obter uma versão diferente do script pela rede a cada tentativa; não altera a
semântica de captura de exceções do motor.

## Formato binário v1

Todos os inteiros são little-endian. Não há compressão, caminhos para extração,
desserialização JSON genérica ou campos opcionais. Strings são `u32` de quantidade
de bytes seguido de UTF-8 estrito. Vetores têm contagem `u32`. Um campo de bytes
tem comprimento `u32` seguido dos bytes. Floats são os bits IEEE-754 `f32` em
little-endian. O arquivo completo tem limite de **12 MiB**.

Cabeçalho fixo de 20 bytes:

1. Magic ASCII `NOIRSNAP` (8 bytes).
2. Versão do formato `u32`, atualmente 1.
3. `browser_ipc::PROTOCOL_VERSION` como `u32`.
4. Comprimento `u32` do restante do arquivo.

Payload, nesta ordem:

1. Versão do pacote CLI (máximo 128 bytes) e `NOIR_BUILD_ID` (máximo 256 bytes).
2. Storage: tag `u8 = 0` (Unavailable), contagem `u32 = 0`.
3. Largura e altura do viewport, `f32`, finitas e entre 64 e 8192.
4. URL final (máximo 8192 bytes), HTML (máximo 1 MiB).
5. Imagens (máximo 32): ID, largura/altura `u32`, dimensões naturais `f32`, RGBA.
6. Metadados de áudio (máximo 8): src, sample_rate `u32`, channels/bits_per_sample/
   block_align `u16`, frames `u64`.
7. Scripts (máximo 4): src e source.
8. Stylesheets (máximo 8): href e source.

O leitor verifica magic, versões, tamanho e identidade antes dos dados de página.
O tamanho declarado deve coincidir com o tamanho do arquivo regular aberto.
Comprimentos e contagens são limitados antes de reservar memória ou ler corpos;
cotas agregadas são descontadas antes da próxima alocação. Nenhum comprimento
controlado pelo arquivo decide uma alocação sem limite. O leitor incremental não
carrega o arquivo inteiro em memória. Ele reutiliza as cotas e os validadores
públicos de imagens/áudio do IPC e aplica as mesmas cotas de scripts do supervisor:

- RGBA: até 4 MiB por imagem e 8 MiB somados, dimensões até 1024×1024 e tamanho
  exato de largura × altura × 4; dimensões naturais segundo o validador IPC.
- IDs de imagem: orçamento total de 1 MiB, até 8192 bytes por ID comum; IDs `data:`
  usam o orçamento maior já permitido pelo IPC. IDs duplicados são recusados.
- Scripts: src até 8192 bytes, fonte até 128 KiB, fontes somadas até 256 KiB,
  sem src duplicados.
- CSS: href até 8192 bytes, não vazio e sem controles, fonte até 128 KiB,
  fontes somadas até 256 KiB, sem href duplicados.
- Áudio: src até 8192 bytes, até 64 KiB somados, metadados PCM validados pelo IPC;
  só documentos `file:` podem conter esses metadados.

URLs não podem conter credenciais ou controles. Somente origens HTTP, HTTPS,
`file:` local sem host/consulta e `noir:` sem host são aceitas. Não se faz DNS nem
canonicalização de caminhos ao ler uma captura. Identificadores de recursos são
dados, não novas permissões de leitura.

Truncamento, payload extra, bytes extras depois do arquivo, UTF-8 inválido,
estrutura inválida, storage não vazio/disponível ou versões incompatíveis causam
erro antes de executar conteúdo. Não existe opção de ignorar incompatibilidades.
O supervisor e renderer continuam validando seus limites normais durante replay.
Um documento que não couber no formato é recusado, nunca truncado silenciosamente.

`NOIR_BUILD_ID=development` é a identificação padrão de desenvolvimento, **não um
hash do código ou executável**. Dois builds modificados com a mesma versão/ID
podem se aceitar. Use um ID de build distinto e registre a revisão ao preparar
um diagnóstico reproduzível. O formato não autentica a procedência: alguém com
acesso de escrita pode alterar dados e metadados. Um snapshot não confere confiança
ao seu conteúdo nem prova que ele veio de um determinado site.

## Privacidade e segurança operacional

Mesmo sem cookies ou perfil, URLs, consultas, HTML, JavaScript, CSS e pixels podem
conter informações privadas, dados de formulário ou tokens embutidos pela própria
página. O arquivo não é criptografado e herda as permissões da pasta de destino.
Use uma pasta privada, não o adicione ao Git nem a uploads automáticos e revise o
conteúdo antes de compartilhar. A captura não promete remover segredos dos bytes
da página, pois isso alteraria o caso reproduzido.

Trate snapshots recebidos como HTML não confiável. Um snapshot remoto continua
isolado; um snapshot local segue as limitações do renderer local já existentes,
sem ganhar acesso ao broker, rede ou caminhos citados pela página. Não edite sua
URL para contornar a exigência de AppContainer. Apagar/redigir conteúdo ou mudar
origem produz outro caso de teste, não o replay fiel do original.

## Verificação

`apps/browser-cli/tests/snapshots.rs` cobre captura opt-in, fixture segura com
script/CSS, replay depois de remover todas as fontes originais, preservação de
viewport, prevenção de sobrescrita, rejeição de arquivos inválidos e replay HTTPS
com URL `.invalid` (sem download), pelo caminho AppContainer no Windows. Fora do
Windows, o teste exige falha fechada para replay remoto.

Os testes unitários do codec cobrem roundtrip byte a byte, Unicode/NUL, RGBA,
metadados de áudio, limites e somas, versões/build, origem, storage, todos os
prefixos truncados de uma fixture e bytes excedentes. Os testes existentes de
subprocesso, sessões, timers e animation frames não são substituídos.
