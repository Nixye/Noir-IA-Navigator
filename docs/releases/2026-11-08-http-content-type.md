# Item 9: Content-Type e charset no broker HTTP

Estado: implementação parcial; revisão textual concluída sem bloqueadores e checagem estática aprovada. Sem testes ou validação funcional. Itens 1-9 ABERTOS/parciais; item 19 adiado. Não foram executados testes, fixtures, aplicativo ou requisições de rede nesta documentação. O executável em dist/Noir não foi reconstruído; não há publicação ou nova distribuição.

## Contrato do recorte

O parser puro de metadados é integrado ao fetch real de browser-core. Content-Type deixa de ser interpretado por prefixos textuais ou por divisão indiscriminada em ponto e vírgula. Isso é um subconjunto conservador, não uma implementação integral de MIME, Fetch ou MIME Sniffing.

- Exige um único campo Content-Type. Campos repetidos são recusados mesmo quando idênticos; valores concatenados com vírgula também não são uma lista aceita. Ausência ou sintaxe inválida recusa a resposta.
- Aceita essência tipo/subtipo composta por tokens ASCII não vazios, sem espaços ao redor da barra. A comparação usa caixa ASCII normalizada. Espaço e tab são permitidos nas bordas e ao redor de separadores de parâmetros e do sinal de igualdade.
- Documento: igualdade com text/html ou application/xhtml+xml. Aceitar o segundo rótulo não acrescenta parser XML/XHTML conforme.
- CSS: igualdade com text/css. JavaScript clássico e módulos: text/javascript, application/javascript, application/x-javascript, text/ecmascript ou application/ecmascript. Sufixos como text/html-extra ou text/css-malicioso não satisfazem esses gates.
- Imagens: família image/ com tipo/subtipo sintaticamente válidos. O gate de família não garante suporte ao formato nem substitui o decoder e suas verificações existentes.
- Parâmetros aceitam token ou quoted-string com aspas duplas e escapes. Ponto e vírgula dentro de aspas não separa parâmetros. Aspas ou escapes incompletos, controles proibidos e parâmetros malformados recusam todo o campo, inclusive quando o parâmetro não é charset. Não há recuperação tolerante de sintaxe quebrada.
- Nomes de parâmetros são insensíveis a caixa ASCII. Para charset, remove-se espaço/tab nas bordas do valor desescapado; o primeiro valor não vazio formado por token ASCII vence. Valores vazios ou que não formem esse token são ignorados, permitindo um charset posterior utilizável. Um rótulo desconhecido, mas sintaticamente utilizável, vence e não é substituído por outro charset posterior.
- O rótulo selecionado usa encoding_rs; ausente ou desconhecido conserva fallback UTF-8. A chamada de decode existente mantém reconhecimento de BOM, que pode prevalecer sobre o rótulo, e substituição de sequências inválidas. Não acrescenta detecção por meta HTML, @charset CSS ou sniffing de conteúdo.

Falhas de metadados ou MIME incompatível recusam o documento principal e omitem subrecursos pelo fluxo existente. O parser é usado para metadados da resposta final consumida; não transforma respostas de redirect em documentos nem substitui a validação de cada destino.

## Limites e fronteiras

O campo é limitado a 4096 bytes antes da cópia/interpretação feita por este parser, com no máximo 32 parâmetros e varredura linear. Todos os parâmetros contam, inclusive desconhecidos e charset repetidos. O limite não controla alocações anteriores realizadas pelo cliente HTTP ao receber os cabeçalhos, nem é um limite global do conjunto de headers.

Permanecem budgets de bytes, quantidade de recursos e timeout existentes. A decodificação textual conserva alocação antes da verificação final de tamanho: esse check final não limita a memória transitória do decoder. O parser limitado não constitui streaming ou decodificação incremental.

Permanecem ensure_verified e AppContainer verificado, DNS público pinado no transporte, TLS padrão, portas 80/443, redirects revalidados, bloqueio de conteúdo misto e regras de origem de módulos. O [ADR 0008](../adr/0008-authorized-subresources.md) autoriza recursos públicos de CDN para CSS, imagens e clássicos; o recorte não restaura a política histórica de mesma origem do ADR 0004. Nenhuma nova capacidade de arquivos/rede em web-*, permissão, canal IPC ou destino é concedida.

## Lacunas e evidências

Não implementa CORS/CSP/nosniff completos, Fetch/XMLHttpRequest JavaScript, cookies, autenticação, cache/revalidação HTTP, streaming, prioridades, cancelamento novo ou compressão nova. Não declara compatibilidade com sites modernos nem conclusão do item 9.

Revisão textual concluída sem bloqueadores e checagem estática aprovada. Resultados de etapas anteriores não validam este recorte. Casos e regressões-fonte descrevem expectativas, não resultados de execução.
## Checagem estática registrada

Na integração, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. A sessão 37339 terminou com exit 0 em 1,06 s; permaneceu somente o warning preexistente de Stmt::Function nunca construído. Revisão textual final concluída sem bloqueadores.

As regressões-fonte não foram executadas. O caso de expansão da decodificação verifica uma saída três vezes maior, mas não exercita diretamente a recusa pelos consumidores; os guards pós-decode desses consumidores foram conferidos por leitura. Isso não demonstra limite de memória transitória nem resultado funcional observado.

Não foram executados testes, aplicativo, requisições de rede, provas de sandbox ou Clippy nesta etapa. Checagem estática e revisão textual não comprovam conformidade ou compatibilidade funcional. O item 9 permanece ABERTO/parcial; o item 19 continua adiado.