# Aceitação e validação — Noir 0.4

Registre o resultado observado, ambiente e revisão junto ao artefato. A presença de uma verificação nesta lista não significa que ela foi executada ou aprovada. Os resultados efetivos pertencem aos relatórios em `docs/validation`.

## Verificações automatizadas

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

`scripts/test.ps1` reúne os checks normais. A CI define verificações Windows e Ubuntu; não apresentar seu resultado remoto antes de executar o workflow. A suíte precisa exercitar HTML/CSS malformados, Unicode, limites de trabalho, geometria, imagens, protocolo truncado/excessivo, versões/IDs, origem, destinos privados, arquivos fora da raiz, falhas de workers, persistência do perfil e navegação transacional.

O runtime JavaScript próprio já existe. Seus testes e os bindings DOM verificam o subconjunto de linguagem, mutações, rollback, listeners e timers. Eles não são Test262 nem demonstram conformidade completa com ECMAScript. Testes de sessão precisam conservar o mesmo VM/DOM entre a carga, eventos e avanço de relógio; renderizar novamente uma cópia estática não valida interatividade persistente.

A navegação remota exige testes de AppContainer reais: token, capabilities, arquivo particular, escrita externa, conexão de rede e criação de filho. Um teste separado deve rejeitar conteúdo remoto no worker comum. Uma falha não pode selecionar um caminho sem isolamento para tornar o teste verde. No Linux, remoto deve continuar recusado enquanto o backend equivalente não estiver verificado.

## Inferência real

```powershell
./tools/ai/setup.ps1
cargo test -p ai-runtime -- --ignored --nocapture
cargo run -p ai-runtime --example inference-check
```

O teste ignorado na suíte normal exige os pesos verificados. Ele confirma geração não vazia, tokens reportados pelo backend, PID independente e encerramento/coleta. A avaliação separada registra respostas e tempos reais em português; não substitui uma avaliação de qualidade ampla. Não usar uma resposta fixa, servidor falso ou indicador decorativo como evidência de inferência.

## Inspeção da interface

1. Abra a inicial nativa. Confira busca, favoritos, espaço ativo, ícones, contraste, foco e tipografia. Durante a carga da IA, a interface deve responder; “IA local pronta” só aparece após confirmação do runtime.
2. Envie uma pergunta curta ao modelo incluído. Abra configurações e confira os quatro modos, campo de chave protegido e indicação de destino no modo provedor. Anexar contexto deve ser opt-in por mensagem; trocar página/aba revoga o anexo. Fechar durante a carga ou geração não pode abandonar o serviço em um encerramento normal.
3. Adicione, edite e remova um favorito. Reinicie e confira persistência. Destinos iniciais reais não devem ser interpretados como promessa de compatibilidade completa com esses sites.
4. Abra abas diferentes em Pessoal e Trabalho, alterne espaços e confira URL, seleção e consulta da inicial por aba. Mova uma aba entre espaços e verifique seus dados e a seleção resultante. Crie/renomeie/remova espaços dentro dos limites.
5. Ative restauração e reinicie; depois desative e confira que URLs não fixadas não permanecem no perfil principal nem no backup; abas fixadas devem persistir por escolha explícita. Favoritos, nomes e preferências devem continuar preservados.
6. Abra `fixtures/specimen.html` e confira texto, PNG, `display:none`, rolagem e links. Redimensione, altere zoom e confira a geometria de clique. A página clara não deve ser recolorida pelo tema escuro da shell.
7. Redimensione a inicial, confira favoritos em diferentes larguras e ative Reduzir animações. Decoração deve ceder espaço e reduzir seu movimento sem bloquear busca e navegação.
8. Abra e feche abas e espaços repetidamente, observando processos e memória. O limite de 16 sessões precisa produzir erro recuperável, sem ultrapassar o orçamento para corresponder ao limite maior de abas salvas.

## Navegação Web e Google

No Windows, abra `https://www.google.com.br/`. Confirme carregamento pelo broker, origem remota correta e renderização real; não aceite uma página interna reproduzindo a marca do site como evidência. Digitar um domínio deve normalizar para URL e termos livres devem construir a busca padrão Google.

Em seguida execute uma busca real. O estado conhecido inclui páginas de resultados que dependem de JavaScript e redirecionamentos ainda não suportados. Registre esse resultado como limitação de compatibilidade, sem marcar “Google Search funcionando” por ter recebido HTTP 200 ou carregado a homepage. A mesma distinção vale para qualquer site real.

Teste recusa de `javascript:`, `data:`, credenciais em URL, destino privado/loopback, tentativa remota de abrir `file:` e downgrade HTTPS. Erros devem preservar o controle da shell e permitir outra navegação. Voltar, avançar, recarregar e respostas tardias não podem aplicar o documento de outra aba ou substituir uma navegação mais recente.

## JavaScript, controles e timers

Use um documento local controlado do subconjunto suportado, com contador atualizado por `click`, valor de `input` observado por seu listener e uma atualização via `setTimeout` após um atraso positivo. Confirme atualização visual após os eventos, não apenas uma mensagem de console. Faça mais de um clique e mais de uma edição para confirmar que a sessão preserva estado.

Troque de aba e de espaço antes de retornar. O VM/DOM deve permanecer associado ao mesmo documento. Grupos em segundo plano têm relógio/tarefas controlados; a recuperação de tempo é limitada a 60 segundos. Isso não promete execução em tempo real de cada timer de fundo, nem um event loop Web completo. Navegar ou fechar a aba encerra a sessão antiga e suas tarefas não podem alterar o documento novo.

Sintaxe, módulo ou API não implementados precisam produzir diagnóstico ou falha previsível. Ausência de implementação não pode ser mascarada com conteúdo simulado, execução em outro motor ou afrouxamento dos limites de instruções/memória.

## Saída sem janela e captura nativa

```powershell
cargo run -p browser-cli -- render noir:specimen --width 1024 --height 768 --output specimen.json --ppm specimen.ppm
cargo run -p browser-desktop -- noir:welcome --capture "$PWD/target/validation/noir-v2-home.png"
```

Compare display list e PPM sob condições fixas. PID, tempos e memória variam e precisam ser excluídos de snapshots de igualdade. O CLI rasteriza conteúdo em CPU; não reproduz os controles nativos da inicial.

A captura PNG exige caminho absoluto e sessão gráfica. Ela espera o primeiro quadro renderizado, captura o framebuffer egui/wgpu e fecha o aplicativo, mesmo sob oclusão do desktop. Não certifica carga completa da IA, toda a navegação ou comportamento temporal de animações. Registre viewport, DPI, backend e versão. Teste uma sessão gráfica Linux antes de afirmar validação visual de ponta a ponta nessa plataforma.

## Requisitos antes de um candidato público de uso geral

- Revisão independente do launcher AppContainer, broker, IPC, scripts, imagens e runtime; resolver achados críticos.
- Backend Linux equivalente verificado antes de liberar remoto; ampliar testes de isolamento em versões de Windows.
- Conjuntos selecionados e elegíveis de WPT/Test262, com passes, falhas e exclusões explícitos.
- CSP/CORS, APIs Web, origem, cookies, certificados, cache e armazenamento avaliados conforme o escopo oferecido.
- Fuzzing com corpus, sanitizers quando aplicáveis, regressões de crash e navegação prolongada.
- Avaliações de acessibilidade, teclado, DPI, internacionalização, GPUs, memória e desempenho controlado.
- Avaliações de qualidade/prompt injection da IA e supervisão sob crash; autorização específica antes de ferramentas que produzam efeitos.
- Licenças dos recursos de IA/dependências, instalador, assinatura e atualização autenticada com rollback.

Esses requisitos não são substituídos por um pacote executável ou pela existência de testes unitários. A matriz e a comunicação do produto precisam acompanhar as evidências realmente obtidas.

Na alteração 0.3, a suíte de testes não foi executada a pedido do usuário. As verificações acima permanecem critérios de engenharia; consulte o relatório 2026-11-02-windows-v3.md para distinguir compilação e prévias do que ainda deve ser avaliado interativamente.

A entrega 0.4 mantém a dispensa de execução da suíte solicitada pelo usuário. O relatório 2026-11-02-windows-v4.md distingue compilação, diagnóstico por páginas reais e comportamentos ainda pendentes de avaliação, incluindo modo total e seus controles de revogação.
