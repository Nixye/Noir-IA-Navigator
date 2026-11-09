# Item 15: codec IPC limitado

Estado: implementação parcial; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores. Itens 1–15 ABERTOS/parciais; item 19 adiado. Sem validação funcional. O executável em dist/Noir não foi reconstruído; sem publicação ou nova distribuição.

## Recorte

O decoder de browser-ipc limita coleções durante Deserialize: 100.000 items e links por RenderedPage, 128 warnings, 256 options por Control e 128 pares em session_storage da resposta e entries do seed. Não confia em size_hint para reservar; solicita crescimento dentro da cota e recusa a presença do elemento N+1 sem desserializá-lo. O seed permanece opcional onde previsto e options conserva default vazio. O visitor de items também recusa mais de 256 controles, após desserializar cada item: o 257º controle pode ser construído, inclusive com até 256 options, mas não é retido. Isso é distinto da recusa de N+1 sem desserialização ao atingir a cota total da coleção.

A saída usa serde_json::to_writer em buffer privado limitado a 48 MiB. Só após serializar com sucesso escreve prefixo u32 LE, corpo e flush. Erro de serialização/excesso deixa o transporte intocado; erro de I/O pode deixar prefixo ou corpo parcialmente enviados. O formato JSON, defaults e versão 7 permanecem inalterados. Validação semântica e commit de versão/ID, página, acesso/storage e checkpoint continuam necessários. Veja o [ADR 0020](../adr/0020-bounded-ipc-codec.md).

## Limites

O frame de entrada ainda é alocado inteiro. Strings escapadas, temporários JSON e elementos permitidos ainda podem alocar antes dos limites semânticos. As cotas aninhadas não formam orçamento global de memória. Result::Err continua limitado pelo frame, sem novo teto próprio menor. O writer não limita o objeto já construído nem CPU/alocações/varreduras internas de Serialize; reservas solicitadas limitadas não são garantia exata de RAM física por causa do alocador.

Não implementa sandbox nova, SOP/CSP completos, política de rede nova, proteção de credenciais ou isolamento adicional do SO. O protocolo NSP1 e a evidência histórica do ADR 0010 não foram reexecutados. Não conclui segurança/isolamento geral nem o item 15.

## Evidências

Checagem estática deste incremento aprovada; revisão estática final concluída sem bloqueadores. Nenhum teste, fixture, aplicativo, navegação, rede, sandbox proof, fmt/check/build foi executado nesta etapa documental. Fontes de teste eventualmente adicionadas não equivalem a execução. Não há validação funcional ou auditoria de segurança.

A checagem final aprovada do item 14 (sessão 77303, exit 0 em 1,45 s, somente warning preexistente Stmt::Function) e sua revisão estática final sem bloqueadores são evidência histórica exclusiva do item 14. Não comprovam este incremento e não foram testes.

## Validação futura, não executada

Verificar limites exatos e N+1 em cada coleção, campos obrigatórios/defaults, contagem de controles, strings escapadas e itens aninhados; verificar rejeição sem desserializar o item excedente. Exercitar serialização válida, falha de Serialize, teto exato/excedente e I/O parcial, observando ausência de bytes no transporte antes do sucesso completo da serialização. Revalidar versão/ID/página/acesso/storage antes de aceitar checkpoint. Esses casos são roteiro futuro, não resultados.

## Checagem estática registrada

Na integração central do item 15, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check da sessão 94876 terminou com exit 0 em 1,92 s; permaneceu somente o warning preexistente Stmt::Function. A revisão estática final foi concluída sem bloqueadores. Nenhum teste, fixture, aplicativo ou sandbox proof foi executado. Compilação não comprova comportamento funcional nem isolamento.
