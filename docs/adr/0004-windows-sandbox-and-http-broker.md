# ADR 0004 — AppContainer e broker HTTP separado do motor

Data: 2026-11-01. Estado: implementado e verificado localmente no Windows; remoto indisponível nas demais plataformas até existir backend equivalente verificado.

## Decisão

O broker privilegiado carrega HTTP/HTTPS e fornece somente bytes ao motor próprio. Um documento remoto só pode ser renderizado dentro de um AppContainer Windows sem capabilities. O processo é descartável a cada navegação. Não há fallback para um processo comum quando o isolamento falha.

Antes da primeira requisição remota, o executável executa uma prova em AppContainer e exige seis resultados: token AppContainer, zero capabilities, leitura de arquivo particular negada, escrita fora do contêiner negada, conexão TCP à internet negada e criação de subprocesso negada. Uma falha é guardada como recusa durante a execução da aplicação. A prova não concede acesso ao conteúdo de arquivos: só transmite booleanos.

O launcher cria um perfil AppContainer exclusivo por worker e uma pasta de runtime temporária. Copia apenas o executável para essa pasta e concede ao SID do contêiner leitura/execução. Não altera ACLs dos documentos, do perfil do usuário nem da árvore do repositório. STDIN/STDOUT são os únicos handles herdados, por uma allowlist explícita. O filho é criado suspenso, recebe um Job com memória de processo de 512 MiB, limite de um processo e kill-on-close, e só então é retomado. A política de criação de filhos também é restritiva no token. A aplicação elimina runtime e perfil após encerrar o processo.

As chamadas Win32 necessárias são o único trecho `unsafe` permitido, isolado em `security-sandbox`. DOM, CSS, layout, JS e IPC continuam sujeitos a `unsafe_code = forbid`. Essa exceção é uma fronteira de plataforma documentada, não uma revisão de segurança independente.

## Rede e recursos

- Somente HTTP/HTTPS, portas 80/443 e URLs sem credenciais.
- DNS validado contra redes privadas, loopback, link-local, multicast e faixas reservadas; os resultados aceitos são fixados no cliente HTTP. O broker não herda proxies do ambiente.
- Revalidação por redirect, até cinco saltos; HTTPS não pode fazer downgrade para HTTP.
- HTML até 1 MiB. Prazo total de rede de 25 segundos, DNS até quatro segundos e conexão até cinco segundos.
- Recursos subordinados exigem a mesma origem, inclusive após redirects. Não há acesso remoto a `file:`, perfis locais ou servidor de IA da máquina.
- Até 12 tentativas de imagem, 4 MiB codificados por arquivo, 1024 × 1024 por imagem e 8 MiB de RGBA no documento.
- Até quatro tentativas de script externo, 128 KiB por script e 256 KiB no total; fontes seguem pelo IPC como dados identificados por `src`, sem I/O no runtime JS.
- Folhas externas são limitadas a quatro tentativas de 32 KiB; o subconjunto atual as incorpora ao final do head, sem alegar fidelidade completa de ordem da cascata ou suporte aos padrões CSS modernos.

Não há cookies persistentes, credenciais automáticas, CORS/Fetch de scripts, cache HTTP completo ou atualização autenticada nesta etapa. URLs de busca são construídas para o Google (`www.google.com.br/search`); isso não implica que o motor consiga executar toda a aplicação JavaScript do Google.

## Verificação e limites

O teste de integração `appcontainer_proves_file_network_and_child_process_denial_before_remote_render` executa a prova real do sistema operacional e renderiza bytes com origem HTTPS em outro PID. Outro teste envia uma origem remota ao worker comum e exige recusa. Testes unitários verificam esquemas, origens e destinos privados/reservados. `cargo run -p security-sandbox --example probe` oferece a mesma prova sem depender de um servidor remoto.

## Sessões interativas

O desktop pode reter até 16 processos de documento simultâneos. Uma sessão mantém o DOM, a VM própria e seus callbacks no mesmo worker, com IDs estáveis para controles. Reflow e cliques não repetem o carregamento de scripts. Cada operação de sessão tem ID explícito, resposta validada e deadline de dez segundos; falha encerra o worker e invalida seu transporte. Fechar ou navegar a aba elimina a sessão. A classe `RendererSession` libera sua vaga por RAII mesmo quando o spawn ou a primeira resposta falha.

Valores de input seguem como pares tipados de ID e texto, com até 256 controles, 32 KiB por valor, 8192 caracteres por valor e 256 KiB totais. A fronteira de bindings verifica a espécie de elemento antes de alterar seu valor. Eles nunca são interpolados em código JavaScript. O supervisor não interpreta conteúdo da página como comando privilegiado.

Timers atrasados do subconjunto JS recebem mensagens `Tick`, com até 60 segundos de avanço por operação. `next_timer_ms` comunica o próximo prazo; a UI entrega o relógio para a aba ativa e limita o trabalho em abas estacionadas. O teste de integração avança49ms, confirma o callback ainda pendente, avança mais1ms e exige a alteração DOM no mesmo PID. Isso é execução do callback retido, sem repetir scripts. O protocolo ainda não equivale a todas as filas/event loops do padrão Web.

No computador de desenvolvimento, a homepage do Google foi carregada via HTTPS e produziu display list e imagem CPU com logo e links. A busca real retornou uma página do próprio Google que exige execução/redirect JavaScript; não foi tratada como resultado de busca bem-sucedido. Não há alegação de compatibilidade universal com sites ou de aprovação em WPT/Test262.

AppContainer reduz acesso a recursos do host; ele ainda permite os recursos próprios do contêiner e arquivos de sistema que o Windows disponibiliza a aplicativos desse tipo. O decoder de imagens e o cliente TLS estão no broker e continuam superfície de ataque. Alteração concorrente do filesystem, exaustão do resolver do sistema, auditoria externa e validação em outras versões do Windows continuam pontos de revisão.

## Fontes primárias

- [Launch an AppContainer — Microsoft](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
- [AppContainer for legacy apps — Microsoft](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-for-legacy-applications-)
- [UpdateProcThreadAttribute — Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
- [CreateProcessW — Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw)
- [reqwest ClientBuilder — documentação oficial da biblioteca](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.ClientBuilder.html)
