# IA incluída e configurações

O Noir inicia com IA local ativada por padrão. O backend incluído carrega **Qwen2.5-1.5B-Instruct Q4_K_M** em um processo CPU separado do navegador, usando llama.cpp. A interface continua disponível durante a verificação e a carga do modelo. O estado “IA local pronta” só aparece depois de o serviço confirmar que carregou o modelo.

## Recursos distribuídos

`tools/ai/manifest.json` fixa as fontes, revisões, tamanhos e hashes. `tools/ai/setup.ps1` baixa e verifica os recursos:

```powershell
./tools/ai/setup.ps1
```

O modelo oficial da organização Qwen usa a revisão `91cad51170dc346986eccefdc2dd33a9da36ead9`; o arquivo tem 1117320736 bytes e SHA-256 `6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e`. O runtime é o nightly `b11960`, indicado pela versão estável `v0.8.0` do llama.cpp. Os arquivos de distribuição oficiais possuem SHA-256 fixado no manifesto, obtido nos metadados do release GitHub.

O script mantém downloads comprimidos em `target/ai-downloads` e os recursos de execução em `resources/ai`. O pacote deve incluir a pasta `resources/ai` ao lado do executável. São necessários o modelo, as bibliotecas do runtime, `manifest.json`, `SHA256SUMS` e as licenças. O runtime é descoberto primeiro ao lado do executável, depois no diretório de desenvolvimento; `NOIR_AI_RESOURCES` permite um diretório explícito para testes/instalações gerenciadas.

O primeiro carregamento verifica o SHA-256 do modelo incluído. A configuração selecionada pelo usuário é separada do perfil de abas, em `ai.json`, no diretório de configuração do Noir. Modelos e runtime não são baixados silenciosamente durante a inicialização: o pacote completo já os fornece.

## Modos disponíveis

| Modo | Comportamento |
| --- | --- |
| IA local incluída | Carrega o Qwen oficial incluído, sem depender de outro aplicativo ou chave |
| Meu modelo GGUF | Carrega um arquivo GGUF escolhido pelo usuário no runtime incluído |
| Servidor local | Usa um endpoint de loopback, como Ollama ou llama.cpp; chave opcional no cofre do sistema |
| Provedor por API | Usa um endpoint HTTPS compatível com chat/completions, modelo configurável e chave no cofre do sistema |

Os endpoints locais aceitam somente loopback, com `localhost` fixado para `127.0.0.1`. Endpoints externos exigem HTTPS. Credenciais dentro de URLs, consultas, fragmentos e redirecionamentos não são aceitos. Nenhuma chave é gravada em JSON ou em mensagens de erro. No Windows, o armazenamento usa o Credential Manager; no Linux, Secret Service/keyring do sistema, sem alternativa em texto simples se o cofre estiver indisponível.

O backend incluído usa uma porta efêmera em `127.0.0.1`, token aleatório de 256 bits, quatro threads CPU, uma slot de geração e contexto de 4096 tokens. Não oferece Web UI ou proxy MCP. A configuração e o cliente não herdam proxies do ambiente. Um handle de encerramento separado do mutex de geração permite matar/coletar o processo mesmo durante uma requisição. Fechar o navegador cancela a carga e encerra o serviço possuído; serviços configurados pelo usuário não são encerrados pelo Noir.

## Conversa e contexto

As conversas permanecem em memória; não são gravadas no perfil ou em arquivos de transcrição. O contexto de página só é anexado após autorização da interface e seleção explícita do checkbox para aquela mensagem. Ao trocar o conteúdo, a seleção é revogada. No modo provedor, a interface informa que mensagens e anexos autorizados são enviados ao provedor escolhido.

O modo opcional **Assistente de navegação** observa texto e controles expostos pela página e permite propostas de navegação, abertura e alternância de abas, seleção, digitação e cliques. As permissões por ferramenta são concedidas na interface nativa para a conversa. No modo granular, cliques precisam de revisão. A opção **Permissão total** concede todas as ferramentas de navegação e executa ações válidas, inclusive cliques, sem solicitar aprovações adicionais. A observação não inclui senhas nem campos ocultos. O loop executa uma ação por resposta, até oito por pedido no modo granular ou 32 em Permissão total, e espera o resultado real antes de continuar. O modelo não tem acesso direto ao navegador: o shell valida a identidade da aba e a geração do documento, descarta propostas antigas e usa o broker existente. Veja o [ADR 0006](../adr/0006-ai-browser-tools.md).

Permissão total vale somente para a conversa. Nova conversa, desativar o Assistente de navegação ou reiniciar/trocar a configuração da IA revogam esse modo. O painel mantém identificação do modo ativo, **Revogar permissão total** e **Interromper**. As validações de observação, esquema, IDs, geração e broker continuam aplicadas; não há ferramenta de terminal ou acesso geral ao computador. Alterar permissões interrompe propostas não despachadas; ações já iniciadas podem concluir sem encadear outra.

Conteúdo de páginas continua sendo dado não confiável, nunca instrução privilegiada ou autorização. A observação é anexada como dado no papel de usuário. O modo exige `response_format: json_object` no serviço compatível; respostas fora do esquema são rejeitadas sem executar ações. Não há percepção visual de imagens ou canvas. O modelo incluído pode errar no planejamento ou no formato de ações; a funcionalidade não equivale a um agente capaz de operar qualquer página.

O modelo de 1.5B tem limitações de raciocínio e precisão. Não há promessa de qualidade equivalente a modelos maiores. Contexto, tamanho da conversa, resposta e tempo de requisição são limitados; entradas que excedam a janela podem receber erro do runtime. O usuário pode selecionar um modelo GGUF mais capaz ou outro endpoint nas configurações.

## Verificação local

Testes de fronteiras de transporte/configuração/cancelamento fazem parte da suíte normal. O teste com pesos reais é opt-in para a CI não depender de um download de aproximadamente 1.1 GB:

```powershell
cargo test -p ai-runtime
cargo test -p ai-runtime -- --ignored --nocapture
```

O teste de integração com pesos reais verifica geração não vazia, contagem de tokens informada pelo servidor, processo independente e encerramento/coleta. Ele valida integração funcional; não substitui avaliações de qualidade. `cargo run -p ai-runtime --example inference-check` executa três perguntas de avaliação em português e registra as respostas e tempos reais, sem critérios artificiais de aprovação. Resultados do modelo menor inicialmente avaliado foram inconsistentes, motivando a seleção do modelo de 1.5B.

Em 2026-11-01, o modelo de 1.5B gerou “Bom dia!” no teste de integração (5 tokens). A avaliação separada respondeu “Paris” à pergunta sobre a capital da França (2 tokens), “12 mais 7 é igual a 19.” (13 tokens) e sugeriu organizar abas em ordem cronológica ou prioritária (37 tokens). Os tempos observados foram 1.08 s, 5.13 s e 11.98 s, respectivamente, com quatro threads CPU. A inicialização com verificação SHA-256 no build de desenvolvimento levou 27.84 s. Essas observações não são um benchmark, não garantem respostas futuras e variam com hardware, carga e geração.

O encerramento normal e a troca de configuração cancelam a inicialização e coletam o serviço possuído. Encerramento forçado do processo principal pelo sistema operacional ainda requer uma política de supervisão por job/cgroup antes de se prometer coleta sob qualquer crash.

## Fontes e licenças

- [Modelo oficial Qwen, Apache-2.0](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF).
- [Release estável llama.cpp v0.8.0](https://github.com/ggml-org/llama.cpp/releases/tag/v0.8.0) e [runtime b11960](https://github.com/ggml-org/llama.cpp/releases/tag/b11960), MIT.
- [Contrato do servidor llama.cpp](https://github.com/ggml-org/llama.cpp/blob/b11960/tools/server/README.md).
- [API local compatível do Ollama](https://docs.ollama.com/api/openai-compatibility).

As cópias `LICENSE-QWEN.txt`, `LICENSE-LLAMA.txt` e `runtime/LICENSE-LLVM-OpenMP` acompanham os recursos. A revisão de redistribuição deve conservar também os avisos das bibliotecas distribuídas pelo runtime.

## Observação do Assistente de navegação: item 17

O snapshot local é textual: texto/display list, controles e links expostos pela shell. Não é uma API completa de DOM/layout/acessibilidade nem captura de imagens. A extração limita a cópia desde a origem. A preparação para o modelo recebe uma referência ao snapshot, sem clonar todo o conteúdo: aplica primeiro as permissões de leitura (campos de página/elementos) e abas (lista de abas). A serialização JSON tem teto de 10.000 bytes UTF-8 durante a escrita, com alvo de redução de 7.000 bytes; caracteres, escapes JSON e bytes não são equivalentes. Falhas retornam erro estruturado antes da requisição, sem enviar prefixos parciais.

Password/hidden são excluídos, mas texto visível, URLs e campos comuns podem conter segredos. O limite de tamanho não é uma garantia de remoção de credenciais. A observação continua sendo dado não confiável; validação e permissões não asseguram imunidade a prompt injection. Escopo aba/geração, grants, Permissão total, ações e timers não mudam. Interromper não desfaz ações já iniciadas.

O [contrato do recorte](../releases/2026-11-08-bounded-ai-observation.md) detalha limites e lacunas. Item 17 parcial; checagem estática aprovada; revisão estática final concluída sem bloqueadores. Não foram executados testes, inferência ou navegação nesta etapa; os resultados históricos acima não validam este incremento.
