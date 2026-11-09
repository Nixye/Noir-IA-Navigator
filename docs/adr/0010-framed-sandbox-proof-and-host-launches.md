# ADR 0010 — Prova AppContainer enquadrada e herança de handles

Status: aceito. Data: 2026-11-03.

## Problema

A interface apresentou `A verificação AppContainer excedeu o prazo (sandbox exit 0x0)`. O probe podia encerrar corretamente e ainda assim o leitor esperar EOF: a prova era JSON lido por read_to_end. Uma ponta de escrita herdada por outro processo mantém o pipe aberto, mesmo depois de o emissor original terminar.

O launcher AppContainer usa CreateProcessW nativo com uma allowlist de handles. Havia uma janela em que seus pipes eram herdáveis pelo processo pai. A inicialização concorrente da IA usava Command::spawn, cujo mecanismo normal de herança não compartilha a coordenação interna com o launcher nativo. Configurar os três stdio como null não impede herdar outros handles marcados. A coordenação interna do Rust não cobre CreateProcessW externo. Referência primária: [implementação Windows de std::process](https://github.com/rust-lang/rust/blob/main/library/std/src/sys/process/windows.rs).

Essa é uma condição de corrida coerente com a falha. O teste isolado do executável gráfico já passava antes da alteração; não se atribui o defeito a uma incapacidade permanente do binário GUI de escrever em stdout.

## Decisão

A prova usa um frame interno: magic `NSP1`, tamanho little-endian de 32 bits e JSON de até 4096 bytes. Cabeçalho e tamanho são validados antes da alocação; a leitura exata do payload encerra a resposta sem depender de EOF. Campos desconhecidos, campos obrigatórios ausentes e tipos inválidos são recusados. Todos os seis resultados de isolamento continuam obrigatórios.

Os pipes começam não herdáveis. Somente os dois extremos destinados ao worker são temporariamente marcados para a chamada CreateProcessW e a allowlist; a marca é removida no pai antes de o worker ser retomado. Um mutex comum protege essa janela e os três lançamentos normais conhecidos do Noir: runtime de IA, renderer local descartável e sessão local persistente. A trava cobre apenas a criação, não a vida do processo filho. Futuros lançamentos do host devem usar `spawn_host_process`.

A coordenação cobre os lançadores conhecidos do produto, não chamadas arbitrárias de bibliotecas externas ao wrapper. Por isso o protocolo também não depende de todos os escritores fecharem. Falhas de coordenação ou prova continuam bloqueando navegação remota. Não há fallback sem AppContainer, capabilities adicionais, ampliação da allowlist ou aumento do prazo de oito segundos.

## Verificação

Testes mantêm um escritor real de pipe aberto após enviar uma prova completa e exigem resposta antes de EOF. Outros recusam frames truncados, magic/tamanho inválidos, campos ausentes/adicionais e provas negativas. Testes do executável GUI real exercitam a prova do sistema, renderização remota com bytes controlados e criação concorrente de sessões locais/remotas com reflow.

O protocolo IPC de páginas continua na versão 3, independente do frame interno do probe. A política de arquivos/rede/processos, o Job de 512 MiB e um processo, o token sem capabilities e a separação do broker permanecem iguais. Nenhuma capacidade de I/O foi introduzida nos crates web-*.
