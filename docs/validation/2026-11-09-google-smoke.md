# Google no Noir: smoke delimitado de 2026-11-09

## Estado atual após o ciclo 16

O Google continua incompatível. A captura/replay conservou os avisos e revelou erros anteriores ao erro secundário `_DumpException`: descriptors de objetos host/native no script 2 e acesso a propriedade de null/undefined nos scripts 8/9. O timer `src` agora identifica nó 112, tag `script`, valor string. Isso é progresso no diagnóstico, não uma correção da página. Em cenário GUI separado, a consulta `Noir browser test` foi enviada e navegou ao Google Search, mas não apresentou resultados: **busca testada e falhando neste snapshot**. Voltar restaurou a homepage e a posição do histórico, com campo vazio. Login e reprodução de mídia continuam não testados. Os trabalhos posteriores sobre `Math.random`, `performance` e `Symbol` estão em andamento e não são validados por estes resultados.

As seções iniciais abaixo preservam a evidência do ciclo 13. As seções finais registram separadamente o novo binário de captura/replay e o teste GUI de busca do ciclo 16. O replay CPU anterior não submeteu busca.

## Resultado do smoke inicial (ciclo 13)

A homepage de `https://www.google.com/` apresentou logo, campo e botões, mas conservou aviso de compatibilidade e erros JavaScript/CSS. É evidência de **renderização parcial**, não de Google plenamente funcional nem de navegador pronto. Não houve submissão de busca, login ou teste de reprodução de mídia. A causa inicial dos erros com `_DumpException` não foi estabelecida.

Este relatório consolida evidências existentes; sua redação não abriu navegador, fez requisições ou executou novos testes.

## Cenário e identidade

- Data observada no resultado: `2026-11-09T01:54:44.4641721Z`.
- Computador/repositório: DARKFRAME, `S:\IAs\Codex\Noir`.
- Executável do smoke: `target/validation/google-current/noir-cycle13.exe`, cópia do ciclo 13, 83.663.360 bytes.
- SHA-256 conferido: `66C535CCDE8AA1B0621EC6075E332E0B785A0E079D0C344C0A997C4F651A3CC5`.
- Perfil temporário. URL pedida e endereço observado: `https://www.google.com/`; título `Google — Noir`.
- O smoke local anterior abriu `fixtures/compatibility-v06.html` e mostrou `JS ok: in, instanceof, delete, for-in e lacunas de arrays.` e `DOM ok: metadados, atributos e consultas de elementos.`. Isso confirma apenas a fixture controlada e não transfere seu resultado para o Google.
- O executável entregue `dist/Noir/noir-validacao-2026-11-09.exe` tem outro SHA-256, `8B6C0C9E004742C8B3F986B2988F806F44409E3C834838C6FE2E3949A6C1DFDE`, preservado. Este smoke não é um teste desse artefato nem das alterações posteriores de diagnóstico.

## Evidências preservadas

Todos os arquivos abaixo ficam em `target/validation/google-current/`:

- `result.json`: resultado estruturado, identidade, horário, avisos, limites e limpeza.
- `binary-and-session.json`: identificação do binário de origem e perfil temporário.
- `diagnosis.md`: análise do caminho de erro e limites de inferência.
- `local-fixture.png` e `local-fixture-ax.txt`: fixture local.
- `google-home.png` e `google-home-ax.txt`: homepage.
- `google-inspector.png` e `google-inspector-ax.txt`: inspector e avisos.
- `gui-stdout.log`, `gui-stderr.log`, `copy-gui-stdout.log` e `copy-gui-stderr.log`: vazios; ausência de conteúdo nesses logs não significa ausência de erro, pois o inspector registrou as falhas.

O resultado registra fechamento apenas das janelas de validação, sem processo Noir remanescente na checagem daquela sessão. Não é uma garantia sobre processos abertos depois. Os hashes foram conferidos novamente durante esta atualização documental.

## Avisos exatos do inspector

```text
JavaScript script 2 error: Runtime: Value is not callable (_DumpException)
JavaScript script 8 error: Runtime: Value is not callable (_DumpException)
JavaScript script 9 error: Runtime: Value is not callable (_DumpException)
JavaScript timer: Runtime: Unsupported DOM assignment src
Unsupported CSS selectors were ignored
Unsupported CSS at-rule @-webkit-keyframes was ignored
Unsupported CSS at-rule @keyframes was ignored
Unsupported CSS at-rule @if was ignored
```

## O que o diagnóstico permite concluir

### `_DumpException`

O caminho de chamada em `web-javascript` acrescenta o nome da função/propriedade ao erro externo `Value is not callable`. Esse sufixo não é stack trace nem identifica uma expressão anterior. Um erro lançado no `catch` substitui o resultado anterior; o diagnóstico observado não conserva uma cadeia causal suficiente para recuperar esse erro inicial.

O HTML histórico `target/audit-controls/google.html`, de 2026-11-03, contém `catch(e){_._DumpException(e)}` nos scripts 2/8/9. Ele sustenta a hipótese de erro secundário no tratamento de exceção, mas não é a fonte capturada da navegação de 2026-11-09. Não se pode deduzir daí o conteúdo atual dos scripts nem qual operação falhou primeiro. Não há evidência para inventar uma `_DumpException`, preencher `Error.cause` da página ou alterar a semântica do `catch` como correção.

### Atribuição `src`

A mensagem confirma que uma atribuição DOM a `src` foi recusada no timer. O aviso não informa nó, tag, tipo do nó, valor ou localização do script. Assim, não permite distinguir `img`, `script`, `iframe` ou outro destino e não prova falha HTTP de recurso. O diagnóstico analisado recusa essa propriedade antes de um carregamento dinâmico.

### CSS

Seletores não suportados e as regras `@-webkit-keyframes`, `@keyframes` e `@if` foram ignorados. Os avisos comprovam limitações no conteúdo observado, mas não identificam sozinhos quanto cada regra contribuiu para as diferenças visuais. Não são prova de suporte a animações CSS.

## Plano após o smoke inicial (estado histórico)

O plano abaixo foi registrado quando captura e diagnóstico ainda estavam pendentes. O ciclo 16 executou a captura/replay e adicionou contexto de erros, como documentado a seguir; não concluiu os fluxos funcionais nem a investigação causal.

1. Capturar explicitamente, com limites e destino local controlado, o `DocumentInput` da mesma navegação no broker: URL final, HTML decodificado, fontes de scripts, IDs/hashes e recusas de recursos. `browser-cli --output` grava `RenderedPage`, não uma exportação equivalente do HTML/scripts.
2. Melhorar o diagnóstico interno limitado para conservar contexto do erro anterior e identificar o destino da atribuição `src`, sem expor o conteúdo atribuído nem modificar a semântica observável da página.
3. Se houver replay, manter a URL remota original e o caminho de execução pelo AppContainer verificado. Não reabrir HTML remoto salvo como `file://` nem executá-lo por VM/CLI local sem sandbox, pois isso muda a fronteira de confiança.
4. Recompilar e repetir os checks do código alterado antes do novo smoke. Só depois verificar, em cenários próprios, busca, navegação continuada e outros fluxos. A captura/repetição descrita aqui é trabalho pendente, não resultado executado.

No fechamento desse smoke inicial, o [baseline funcional](2026-11-08-functional-baseline.md) registrava a suíte ampla do ciclo 15: 797 aprovados, 0 falhas, 1 ignorado e 0 filtrados. Esse resultado não eliminava as incompatibilidades observadas nem certificava alterações de diagnóstico feitas depois dele.

## Ciclo 16: captura pelo broker e replay com diagnóstico

### Identidade e procedimento

Diretório de evidências: `target/validation/google-diagnostics-cycle16/`.

- `capture-session.json`: captura registrada em `2026-11-09T02:16:05.2027058Z`, entrada `https://www.google.com/`, modo broker sem executar scripts e sem perfil/cookies.
- `browser-cli-cycle16.exe`: 34.610.688 bytes, versão `0.9.0 development`, SHA-256 `672A7D6F29D9497484C13BD010558AC7A1F5C5A99611FC57F5D5CF4050F8F3D7`.
- `google-home.noir-snapshot`: 189.944 bytes, SHA-256 `0E0CA5F51A811D8A6C0C649EFBF70139764CAF18E39DE888C580991D0FC216F6`.
- `capture.log.exitcode` e `replay.log.exitcode`: ambos 0. O log de captura avisa que snapshots podem conter dados privados e devem ser revisados antes de compartilhar; este registro não autoriza compartilhamento.
- `google-home-replay.json`: resultado estruturado com avisos; `google-home-replay.ppm`: saída raster do replay, não captura de uma nova janela desktop.

Comandos confirmados pelo executor, com caminhos abreviados relativamente ao diretório de evidências e usando o mesmo executável nas duas etapas:

```powershell
.\browser-cli-cycle16.exe capture https://www.google.com/ --snapshot google-home.noir-snapshot --width 1024 --height 768
.\browser-cli-cycle16.exe replay google-home.noir-snapshot --output google-home-replay.json --ppm google-home-replay.ppm
```

Uma leitura do cabeçalho/metadados do snapshot confirmou formato 1, IPC 8, versão `0.9.0`, build `development`, URL de documento `https://www.google.com/`, viewport 1024 × 768 e marcador de storage indisponível com zero entradas. Os hashes acima foram conferidos diretamente nos arquivos.

A captura ocorreu no broker; não executou os scripts. O replay consumiu os bytes preservados sem buscar novamente os recursos, mantendo a origem HTTPS e o fluxo de `browser_process::render_in_subprocess`, que exige AppContainer verificado para origem remota. Não houve conversão para `file://`, VM local sem sandbox ou acesso ao perfil real/cookies/storage. O conteúdo era a homepage pública, sem login ou envio de busca. Isso mantém a fronteira de confiança do replay, sem transformar o resultado em auditoria geral de segurança.

### Evidência de execução: avisos do replay

Os oito avisos em `google-home-replay.json` são:

```text
JavaScript script 2 error [phase=execute, kind=classic, node=9, source=inline, schedule=immediate]: Runtime: Value is not callable (_DumpException) [previously caught: Runtime: Host/native exotic descriptors are not implemented]
JavaScript script 8 error [phase=execute, kind=classic, node=99, source=inline, schedule=immediate]: Runtime: Value is not callable (_DumpException) [previously caught: Runtime: Cannot access a property of null or undefined]
JavaScript script 9 error [phase=execute, kind=classic, node=105, source=inline, schedule=immediate]: Runtime: Value is not callable (_DumpException) [previously caught: Runtime: Cannot access a property of null or undefined]
JavaScript timer [phase=timer, source=unavailable]: Runtime: Unsupported DOM assignment src [context: DOM receiver node=112 kind=element tag=script value-type=string]
Unsupported CSS selectors were ignored
Unsupported CSS at-rule @-webkit-keyframes was ignored
Unsupported CSS at-rule @keyframes was ignored
Unsupported CSS at-rule @if was ignored
```

O diagnóstico agora conserva o erro previamente capturado antes de o reporter falhar. No script 2, comprova uma recusa relacionada a descriptors de objetos host/native. Nos scripts 8/9, comprova acesso a propriedade de null/undefined. O aviso do timer permite identificar `script` como tag do receptor naquele replay, mas não revela o valor da string e não é uma falha HTTP: a atribuição `src` continua recusada. Não há localização fabricada de linha/coluna para esses erros de runtime; o timer registra explicitamente `source=unavailable`.

Esses dados superam parte das limitações do ciclo 13. Não determinam, sozinhos, a operação precisa, chave, target ou sequência completa que causou cada erro. Os avisos CSS são os mesmos, e saída 0 do replay significa conclusão da execução com resultado produzido, não ausência de incompatibilidade.

### Hipótese de inspeção estática, ainda não comprovada

Foi reportada uma hipótese envolvendo instalação de `Symbol.dispose` por `defineProperty`. Ela deve ser investigada separadamente: o aviso executado `Host/native exotic descriptors are not implemented` não nomeia `Symbol.dispose` nem demonstra qual descriptor/target chegou à operação. Não há neste ciclo teste de correção que comprove essa hipótese, e o relatório não a classifica como causa exata resolvida.

### Validação e limites restantes

O ciclo 16 passou fmt, Clippy e workspace: **845 aprovados, 0 falhas, 1 ignorado e 0 filtrados**. A cobertura nova inclui 17 testes de diagnóstico JavaScript, 17 de bindings e 14 de codec/integração de snapshots. O [baseline](2026-11-08-functional-baseline.md) conserva as falhas intermediárias de stack, fixtures e lint, suas correções e os resultados posteriores. O teste de inferência real permanece ignorado.

O executável entregue `noir-validacao-2026-11-09.exe` continua com SHA-256 `8B6C0C9E004742C8B3F986B2988F806F44409E3C834838C6FE2E3949A6C1DFDE`, novamente conferido. Não foi substituído por esse CLI; o smoke visual do ciclo 13, o replay CLI do ciclo 16 e o artefato entregue são evidências distintas.

No fechamento desse replay, permaneciam pendentes a reprodução da operação exata por trás dos erros anteriores, os testes de qualquer correção e a repetição do mesmo snapshot, além de busca, login autorizado, navegação continuada e reprodução de mídia em cenários próprios. O replay não avaliou a GUI de desktop/GPU nem submeteu busca; o cenário de busca executado depois está separado abaixo. As mudanças posteriores ao snapshot validado precisam de novos checks, e o objetivo de um navegador funcional continua aberto.

## Ciclo 16: busca pela GUI e retorno à homepage

### Resultado e escopo

O fluxo nativo aceitou a consulta sintética `Noir browser test` e, após clique em `Pesquisa Google`, navegou para uma URL `/search` contendo `q=Noir+browser+test` e `btnG=Pesquisa+Google`, com título `Google Search — Noir`. **Não apareceu uma lista de resultados.** O corpo visível registrado foi: “Se você estiver com problemas para acessar a Pesquisa Google, clique aqui ou envie um feedback.” O aviso de compatibilidade permaneceu. Os links dessa mensagem não foram seguidos.

Portanto, entrada de texto e submissão navegaram, mas a busca Google não foi funcional nesse binário. Não houve segunda consulta, porque a primeira não produziu resultados. Não houve login; nenhum CAPTCHA, consentimento legal ou pedido de permissão do sistema foi observado. O teste não executou scripts de página diretamente nem fez replay local da resposta de busca.

### Identidade e evidências

Diretório: `target/validation/google-search-cycle16/`.

- Resultado observado em `2026-11-09T02:27:21.6928785Z`; lançamento registrado por volta de `2026-11-09T02:24:31Z` com `--temporary-profile https://www.google.com/`.
- Cópia GUI: `noir-cycle16.exe`, 83.783.168 bytes, versão `Noir 0.9.0 (development)`, SHA-256 conferido `58D65BF453DB1A0A928C535E650DABFEF07C40139738F60A1483998E6420DCAB`.
- Origem: `target/debug/noir.exe`, timestamp `2026-11-09T02:18:42.5065701Z`. O registro identifica explicitamente `cycle16-before-native-descriptor-fix`. Edições/builds posteriores não fazem parte deste teste.
- `report.md`, `result.json` e `binary-and-session.json`: procedimento, resultado, identidade e limpeza; a URL completa está nos registros, sem necessidade de copiar seus parâmetros incidentais neste relatório.
- `google-home.jpg`, `query-entered.jpg`, `search-submitted.jpg`, `search-result.jpg`, `search-inspector.jpg`, `back-home-inspector.jpg` e `back-home.jpg`, com os respectivos arquivos `*-ax.txt`: etapas da GUI e observações de acessibilidade.
- `gui-stdout.log` e `gui-stderr.log`: vazios. Isso não elimina os avisos observados no inspector.

Essa cópia GUI é distinta do CLI de captura/replay e do executável entregue. O SHA-256 do entregue foi conferido novamente e permanece `8B6C0C9E004742C8B3F986B2988F806F44409E3C834838C6FE2E3949A6C1DFDE`.

### Avisos exatos da página de busca

```text
JavaScript script 3 error [phase=execute, kind=classic, node=14, source=inline, schedule=immediate]: Runtime: Value is not callable (random)
JavaScript script 4 error [phase=execute, kind=classic, node=16, source=inline, schedule=immediate]: Runtime: performance is not defined
Unsupported CSS selectors were ignored
```

Esses são erros de runtime observados. Não constituem diagnóstico causal completo da resposta de dificuldade de acesso do Google, nem comprovam que corrigir apenas `Math.random` ou `performance` produzirá resultados úteis. Os trabalhos sobre essas APIs e `Symbol` continuam em andamento, sem validação por esse smoke.

### Voltar, histórico e limpeza

O clique nativo em `Voltar` retornou a `https://www.google.com/`, título `Google — Noir`, logo e campo de busca vazio. `Voltar` ficou desabilitado e `Avançar` habilitado. Isso confirma o destino e a posição do histórico nessa sequência; não prova BFCache ou restauração do texto digitado. Não houve clique em `Avançar` registrado neste cenário.

O inspector da homepage restaurada voltou a mostrar os erros dos scripts 2/8/9, o timer `src` no nó 112/tag `script` e os avisos CSS já descritos no replay. Os detalhes estão em `back-home-inspector-ax.txt` e `result.json`.

Apenas a janela própria de validação foi fechada. O registro de limpeza confirma ausência de processos correspondentes remanescentes naquele momento e preservação das janelas da usuária. O caminho normal broker/AppContainer permaneceu sem mudança de política, sem fallback remoto local e sem alteração de firewall/VPN; esse smoke não é uma auditoria independente da sandbox.

A atualização documental leu esses registros e conferiu hashes, sem executar Cargo, rede ou GUI. O próximo resultado funcional exige repetir a mesma busca com um binário corrigido e identificado separadamente. Submissão bem-sucedida e retorno à homepage não equivalem a resultados de busca funcionais.
