# Diagnóstico contextual limitado de scripts

Este recorte melhora a atribuição de erros do lifecycle inicial. Não implementa APIs DOM ausentes, carregamento por `src`, novos callbacks, console do inspector, stacks JavaScript ou suporte completo a sites. Os scripts continuam sendo exclusivamente bytes já fornecidos pelo broker. Não há rede nem acesso a arquivos no host.

## Origem e fase

Os avisos de preparação e execução registram o índice original do elemento `script` (base 1), o NodeId estável do DOM, `classic` ou `module`, a origem `inline` ou URL/src saneada, e o agendamento `immediate` ou `defer`. Os metadados são capturados quando o script é visitado e acompanham sua fonte congelada na fila. Alterar `src`, texto ou conectividade de um script já preparado não reatribui seu erro a outra fonte. A numeração conserva a posição no snapshot inicial, inclusive lacunas de elementos ignorados.

A origem é identidade de diagnóstico, não autorização. A preparação ainda exige correspondência com o asset do broker, e módulos continuam sujeitos às regras de origem e ao registro autorizado. Um aviso de preparação pode identificar uma fonte solicitada que não foi fornecida. URLs relativas usam a URL de documento autorizada quando disponível. Sem ela, somente o caminho relativo normalizado é exibido, sem inventar uma origem de rede.

`phase=preparation` e `phase=execute` distinguem a etapa do lifecycle. Erros de timers e callbacks iniciais têm sua fase e `source=unavailable`: o vínculo entre funções persistidas e seus arquivos não está implementado, portanto não é inferido a partir do último script executado. O tratamento de `async` continua explicitamente não suportado.

## Posições reais, não inferidas

O byte offset usa base 0 e UTF-8. Linha e coluna usam base 1; coluna conta escalares Unicode, não bytes nem unidades UTF-16. CRLF conta como uma quebra; CR, LF, U+2028 e U+2029 também são reconhecidos. Offset fora da fonte ou no meio de um escalar UTF-8 não recebe linha/coluna.

Para scripts clássicos, a conversão só ocorre quando `JsError::input_source_offset()` comprova que o erro veio diretamente do parser da entrada principal. Um `offset` isolado não basta: JSON, `eval`, `Function`, validação e link de módulos podem usar outros buffers ou posições sintéticas. A fase `execute` descreve a operação do host, inclusive quando ela falha no parse inicial.

Na preparação de módulos, erros do parse feito por `module_requests` recebem a URL do módulo efetivamente analisado e, quando disponíveis, byte/linha/coluna dessa fonte. Assim um erro em uma dependência não é atribuído ao root. O cabeçalho continua identificando o elemento que iniciou a preparação. Outros erros de registro, link e execução não recebem uma posição fictícia na fonte do root; o offset bruto que o erro já possuía pode continuar no seu texto, sem alegar correspondência com o arquivo inicial. Não são acrescentados trechos nem cópias da fonte ao relatório.

## Contexto de operações DOM ausentes

`get` e `set` que já terminavam em `Unsupported DOM property` ou `Unsupported DOM assignment` continuam falhando. O contexto interno acrescenta NodeId, categoria (`element`, `text` ou `document`), tag de elemento e, em atribuições, apenas a variante conhecida do valor (`string`, `object`, `function` etc.). Isso distingue, por exemplo, atribuições a `img.src`, `script.src` e `iframe.src` sem habilitá-las.

Nenhuma propriedade, getter, `toString` ou coerção é executada para produzir esse contexto. Não são incluídos atributos `id`, conteúdo textual, URL/valor atribuído, representações `Debug` de objetos nem valores secretos. Handles inválidos continuam sendo recusados pela validação existente, antes da coleta de contexto.

O contexto usa `JsError::add_diagnostic_context` e aparece somente na representação diagnóstica do erro. `.message`, o valor lançado e os argumentos JavaScript de `window.onerror` não recebem esses rótulos. Nomes de propriedade, inclusive os longos ou com controles, preservam a mensagem JavaScript anterior. Somente sua exibição diagnóstica é limitada e saneada. O contexto pode acompanhar um erro anterior capturado quando uma falha secundária sai do `catch`, pelo mecanismo limitado de `web-javascript`.

## Limites e privacidade

- Permanecem os limites de 64 scripts iniciais, 512 KiB agregados de fonte e 32 avisos de lifecycle.
- Cada aviso enriquecido é escrito diretamente em um buffer de no máximo 2048 bytes UTF-8. O texto completo de um erro não é formatado em uma alocação intermediária sem limite.
- A URL de entrada do rótulo é verificada antes do parse/cópia: acima de 8192 bytes, exibe-se apenas um marcador. Rótulos de origem têm no máximo 256 bytes; rótulos de módulo, 96 bytes antes de compor o contexto.
- Apenas URLs hierárquicas HTTP, HTTPS e file são exibidas. Userinfo, query e fragmento são removidos antes da formatação. Esquemas opacos como `data:` não revelam sua carga. Isso vale para origens e mensagens de asset ausente geradas pelo host, não constitui um filtro geral de segredos em mensagens arbitrárias lançadas pela própria página.
- Tags têm no máximo 32 bytes no contexto. Propriedades conservam a mensagem JavaScript anterior, mas sua exibição respeita o limite global do aviso. Truncamento preserva limites UTF-8 e usa `...`. Controles e marcadores direcionais são neutralizados nos rótulos/avisos.
- Os rótulos internos de `JsError` mantêm seus próprios limites: no máximo quatro, 128 bytes por rótulo e 256 bytes agregados. Não conservam referências a objetos do heap.

Metadados não consomem novas permissões nem relaxam quotas de execução. Recursos continuam fatais e não recuperáveis por `catch`; rollback, desativação de scripts e ordem de eventos/timers permanecem nos caminhos existentes. O relatório não passa a incorporar mensagens de `console.*`; um canal separado de console continua fora deste recorte.

## Cobertura

`crates/web-bindings/src/diagnostics_tests.rs` cobre posições Unicode/CRLF, limites de strings e avisos, remoção de credenciais/query/fragmento, origem inline e externa, congelamento de defer, parser de dependência de módulo, ausência de posição fictícia em JSON/eval/Function/link/runtime, receivers DOM por tag, handles inválidos, valores UTF-16 sem coerção, callbacks de timer e ausência de conteúdo/valores no contexto. Os testes existentes de lifecycle continuam responsáveis pela ordem, quotas, rollback de módulo e fatalidade de Resource.

A execução integrada dos testes é registrada no relatório da entrega; este documento descreve o contrato, não afirma compatibilidade funcional com Google nem validação de navegação remota.
