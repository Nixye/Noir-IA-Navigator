# Modelo de ameaças — broker HTTP e sessões isoladas

Atualizado em 2026-11-08. Escopo: shell, broker local/HTTP, sessões VM/DOM retidas, IPC v7 e AppContainer Windows. Esta análise orienta implementação e testes; não é uma auditoria independente.

## Ativos e fronteiras

Os ativos principais são arquivos, credenciais, disponibilidade da interface e integridade da navegação. HTML, CSS, scripts, imagens, controles e URLs são entradas não confiáveis. Broker e interface possuem os privilégios do usuário. O renderer remoto Windows recebe somente pipes e executa em AppContainer sem capabilities; workers locais não devem ser confundidos com essa sandbox. Linux falha fechado para remoto até haver backend verificado.

O usuário concede acesso ao documento ao digitar seu caminho. Um recurso ou link dentro dele não recebe a mesma autoridade para escolher qualquer arquivo do sistema. A autorização de recursos depende da raiz do documento e da resolução canônica.

## Ameaças e controles atuais

| Ameaça | Controle da fundação | Risco residual |
| --- | --- | --- |
| URL ou redirect malicioso | HTTP/HTTPS80/443 sem credenciais, DNS público validado/fixado, redirects revalidados, sem downgrade HTTPS | Política conservadora não equivale a CORS/CSP/SOP completos de navegador |
| Worker remoto tenta ler arquivos/rede/criar filhos | AppContainer sem capabilities; provas reais de arquivo, escrita, TCP e processo; Job de processo único | Windows permite recursos próprios do contêiner e arquivos de sistema acessíveis a esse tipo de app; auditoria pendente |
| URL tenta acessar LAN, metadados ou IA local | Filtro de IPs e nomes reservados, inclusive loopback/privados; recursos subordinados da mesma origem | Resolver do SO pode reter tarefa até encerrar; novos canais precisam de revisão |
| Caminho de rede ou de dispositivo explícito | Rejeição de hosts em `file:`, UNC e prefixos de dispositivo | Volumes montados ou mapeados dependem do ambiente do usuário |
| Travessia por imagem/link | Resolução canônica e contenção na raiz autorizada | Condições de corrida no filesystem exigem futura abordagem por handles/capacidades |
| Arquivo excessivo | Leitura limitada e recusa do excesso | A autorização do filesystem continua sendo do usuário |
| Imagem maliciosa | Formatos restritos, limites de dimensão/alocação/bytes | O decoder roda no broker e integra sua superfície de ataque |
| Parser, VM ou layout abusivo | Limites DOM/CSS/VM, dez segundos por operação, até16 sessões; Job remoto512MiB | Orçamento agregado pode ser significativo; não equivale a prevenção completa de DoS |
| Filho trava ou falha | Timeout, encerramento, coleta, Job kill-on-close e criação de filhos restrita no token remoto | Worker local usa proteção estrutural anterior e rlimits Linux |
| Resposta IPC forjada | Framing limitado, versão/id e validação de payload/geometria | O protocolo e seus validadores precisam de fuzzing contínuo |
| Segredos em variáveis de ambiente | Ambiente do filho reduzido | Bibliotecas e o SO podem manter outras fontes de informação acessíveis |
| Valores digitados viram código | Controles seguem pares tipados ID/texto; bindings validam espécie e limites antes de atribuir dados | A página pode ler seus controles conforme os APIs JS implementados |
| Conteúdo executável na página | VM própria limitada, rollback em erro e nenhum I/O privilegiado | Subconjunto não é compatibilidade universal; parser/decoder continuam relevantes |
| Prompt injection | Contexto tratado como dado; gateway por aba/geração e confirmação sensível de uso único | Novas ferramentas de agentes exigem avaliações antes de expansão |

## O que ainda falta para isolamento de produto

Antes da primeira requisição remota, a prova exige simultaneamente token AppContainer, zero capabilities, leitura e escrita particulares negadas, TCP internet negado e criação de processo filho negada. Uma falha é uma recusa, sem fallback para processo comum. O launcher copia apenas o executável para runtime temporário com RX do SID; não amplia ACLs de documentos, repositório ou perfil. O filho inicia suspenso com allowlist de handles e só retoma após receber Job512MiB/processo único/kill-on-close. Perfil e runtime são exclusivos do documento.

A exceção FFI `unsafe` fica isolada em `security-sandbox`; o restante do workspace mantém `unsafe_code = forbid`. Sessões retêm DOM/VM e callbacks e exigem versão/id por operação; reflow não repete scripts. Workers locais Linux têm memória1GiB e core dumps desativados. O worker CLI descartável também tem CPU8s; sessões usam deadline por operação, evitando limitar toda a vida da aba a oito segundos cumulativos. AppContainer não concede garantia absoluta sobre todos os arquivos de sistema; revisão externa continua necessária.

O processo deve falhar fechado se o isolamento exigido para uma capacidade não puder ser aplicado. O produto não deve liberar navegação remota por detectar apenas que o renderer está em outro PID.

Ainda faltam sandbox Linux, isolamento adicional do decoder/broker, CORS/CSP/Fetch completos, cookies/cache completos, política avançada de certificados, atualização autenticada, auditoria de dependências, fuzzing e revisão independente. TLS usa a validação padrão de certificados do rustls. A restrição atual de recursos à mesma origem é uma política conservadora do broker, sem alegar um navegador Web completo. Consulte o [ADR0004](../adr/0004-windows-sandbox-and-http-broker.md) para fontes e limites concretos.

## Relato e correção

Registre entradas que causem crash ou consumo excessivo como fixtures de regressão, reduzindo dados pessoais. Documente impacto, fronteira afetada, causa e verificação da correção. Não inclua credenciais ou conteúdo privado no repositório. Até haver um canal público de segurança definido, mantenha a distribuição limitada ao escopo de engenharia e aos documentos controlados.

## Superfície WebAssembly em implementação — 2026-11-08

O primeiro recorte do item 2 adiciona parsing/validação e interpretação de bytes não confiáveis em web-wasm/wasmi 2.0.0, sem JIT/WASI e sem imports privilegiados. Isso amplia a superfície de dependências e não constitui nova prova de sandbox. Features mínimas, quotas estruturais/bytes antes de alocação, fuel ligado ao orçamento JS, pilha limitada e recusa de estado mutável reduzem a superfície; fuzzing, auditoria e validação funcional continuam pendentes. A API Rust consumida é segura, mas o lint unsafe_code do workspace não audita o código transitivo.

O rollback do ADR 0007 impede compartilhar Store mutável entre snapshots. Instâncias elegíveis não possuem estado mutável persistente; Module/Engine imutáveis podem ser compartilhados e Store só vive durante cada chamada. Traps ordinárias não anulam efeitos JS anteriores; Resource restaura a operação e conserva orçamento consumido. Não há aquisição de bytes por novo IPC/rede, processo ou thread da página. Revisão textual concluída e checagem estática aprovada. Veja o [ADR 0016](../adr/0016-bounded-stateless-webassembly.md).

## Dados sessionStorage no IPC — item 11

O [ADR 0017](../adr/0017-bounded-session-storage.md) registra nova fronteira de dados, não autoridade: seed/snapshot de pares UTF-8 limitados no IPC v5. O supervisor atribui origem HTTP(S) canônica pela URL final do broker e identidade da sessão; o renderer só recebe o bucket corrente. file/noir/data/origem opaca/ausente não recebem storage nem compartilham bucket null. Nenhum I/O, credencial ou permissão é concedido.

Requisições são validadas nos dois extremos; respostas só atualizam checkpoint após validar versão/ID, resultado, página inteira, acesso e pares. Duplicatas, excessos e entradas em Unavailable/QuotaExceeded são recusados. O frame de 48 MiB antecede desserialização. O item 15 antecipa cotas das coleções de storage durante Deserialize; strings e buffers JSON ainda podem alocar antes das quotas semânticas menores. Permanecem superfície de alocação/transporte, cópias e risco residual de DoS; quotas de storage não são teto completo de heap.

Limites: 128 pares/64 KiB por origem, 16 origens/aba, chave 1 KiB e valor 16 KiB UTF-8, sem substituição de surrogate isolado. Sem eviction; origem nova vazia não consome slot e bucket existente esvaziado continua contado. Resource restaura estado da operação; quota ordinária preserva o estado anterior à escrita recusada. Resposta inicial obsoleta não confirma storage. Checkpoint de operação em andamento de sessão comprometida pode ser retido antes da navegação seguinte, sem publicar página antiga. Crash/timeout não recupera writes não confirmadas.

Buckets não entram em perfil/disco, logs ou contexto direto IA; fechar aba/reiniciar elimina. A própria página ainda pode mostrar valores no DOM acessível, como outros dados da página. Sem sincronização entre abas, persistência, eventos storage, workers ou offline. Revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada; testes, fuzzing e validação de segurança/funcional não executados neste recorte documental.

## Parsing privilegiado de metadados WAV locais — item 13

O [ADR 0018](../adr/0018-brokered-local-audio-metadata.md) registra uma nova classe de arquivo local lida pelo broker e dados no IPC v7. A raiz autorizada, sandbox, permissões e rede não se ampliam; renderer não recebe PCM/handles nem acesso a dispositivo. O conteúdo continua não confiável. RIFF/PCM é analisado com limites de tamanhos, chunks e aritmética, mas o parser privilegiado acrescenta superfície a revisar e submeter futuramente a malformados/fuzzing.

Até oito tentativas incluindo falhas/repetidos, 4 MiB por arquivo aceito e 8 MiB realmente lidos incluindo inválidos e sentinelas, sem reembolso. EOF observado é obrigatório; esgotamento agregado antes de EOF recusa sem leitura extra, mesmo no fim exato. Arquivo alterado durante leitura não é snapshot atômico; a TOCTOU preexistente canonicalização/open não é resolvida por este recorte.

Validação independente de IPC restringe contagem, IDs src até 8192 bytes cada, soma dos IDs de até 64 KiB, duplicatas, parâmetros/relações PCM e documento file:. O teto de frame não elimina alocações JSON anteriores à validação semântica. Seed único em nós originais e invalidação permanente após set/remove src impedem reutilização por clone/nó novo ou restauração do mesmo atributo. preload é gate reversível sem novo I/O. Rollback Resource restaura mapa/DOM; erros ordinários preservam efeitos. Não é nova prova de sandbox nem suporte a reprodução/mídia remota. Check do item 13 aprovado; revisão final concluída sem bloqueadores, sem testes ou validação funcional.
## Item 14: digest local limitado

O [digest restrito](../specifications/crypto-digest-subset.md) aceita dados não confiáveis e calcula SHA-256/384/512 localmente. A cópia considera somente BufferSource real e a janela interna, antes de getters/coerções do algoritmo. Quotas de 256 KiB por entrada, 2 MiB acumulados e 512 tentativas, instruções proporcionais e orçamento de ArrayBuffer contêm custo síncrono; GC/rollback não devolvem acumulados. Promise não significa trabalho em background. Erros Resource permanecem fatais.

A capacidade exige URL top-level HTTPS fornecida pelo supervisor, com host válido e Host negando por padrão. A URL congela antes da primeira execução, inclusive falhas; rollback não desfaz o congelamento. sourceURL, DOM e propriedades JS não promovem contexto. HTTP/file/noir/data/ausente são excluídos. O [ADR 0019](../adr/0019-bounded-crypto-https-context.md) não afirma implementar ancestors/secure contexts completos e não amplia rede, arquivos ou sandbox.

Hashes não autenticam remetentes e não substituem TLS ou assinaturas. RNG, chaves, encriptação, assinaturas, WebRTC, WebAuthn e dispositivos continuam ausentes. Checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores; sem execução funcional, auditoria ou medição de canais laterais neste incremento.
## Item 15: redução parcial da superfície de alocação IPC

O [ADR 0020](../adr/0020-bounded-ipc-codec.md) antecipa cotas de items/links (100.000 cada), warnings (128), options (256) e snapshots/seeds de storage (128 pares). O visitor ignora size_hint e solicita crescimento dentro da cota; ao atingir N recusa a presença de N+1 sem desserializá-lo. Também conta controles em items, recusando o 257º após construí-lo, sem retê-lo. Cada options desse controle ainda pode ter sido construído até sua própria cota.

O writer privado limita o JSON de saída a 48 MiB durante serde_json::to_writer; somente após sucesso escreve prefixo u32 LE, corpo e flush. Falha de serialização, cota ou reserva não toca o transporte; falha de transporte pode deixar saída parcial. Não substitui validação semântica de versão/ID, página completa, acesso e storage antes de commit do checkpoint. Wire v7 e defaults permanecem iguais.

Risco residual: frame recebido inteiro ainda alocado; strings escapadas, temporários JSON e elementos permitidos podem alocar antes das quotas semânticas. Cotas aninhadas não são orçamento global; arredondamento do alocador impede tratar capacidade solicitada como RAM física exata. Result::Err permanece limitado pelo frame, sem cota própria menor. O writer não limita objetos já construídos nem CPU, alocações e varreduras internas de Serialize. DoS agregado e fuzzing continuam pendentes.

Não há nova prova NSP1/AppContainer, SOP/CSP, segurança de credenciais, isolamento do SO ou política de rede. O ADR 0010 conserva somente seu contexto e evidência histórica. Item 15 ABERTO/parcial, itens 1–14 parciais e item 19 adiado; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores, sem testes ou validação funcional.
