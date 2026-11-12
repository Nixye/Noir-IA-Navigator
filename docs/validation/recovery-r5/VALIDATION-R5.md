# Validação local Noir recovery-r5

## Resultado e base

PASS local default/offline na tentativa 4, encerrada em 2026-11-12 às 10:48:49 UTC: **1502 testes aprovados, 0 falhas, 1 ignored**, 43 resumos de suíte. Os 52 testes novos passaram. Não se somam tentativas nem se conta ignored como aprovação.

R5 continua a V13 restaurada via novos blocos R1–R4. **V14–V49 não foram recuperadas** e seus resultados históricos não são resultados desta base. As fontes R1–R4, suas cópias congeladas e quatro ZIPs, além da V13 restaurada, foram reconferidos e preservados.

Este pacote contém 1098 arquivos de fonte, 9657299 bytes de fonte, mais este relatório e CHECKPOINT-MANIFEST.json. A cópia congelada R5 é idêntica byte a byte e em modos ao mapa da tentativa aprovada. O manifesto externo, não os manifestos herdados, identifica as fontes atuais.

## Ganho demonstrado

A fixture local bounding-panel-recovery-r5.html chama getBoundingClientRect como primeira capacidade nova. Na R4 o script para nessa chamada; na R5 o popup é posicionado a partir das medições reais. Isso corrige um impedimento concreto de scripts que ancoram painéis a elementos. Não demonstra abertura de um site público inteiro.

O bloco acrescenta:
- Registro de posições documentais finais e border-box principal independente de pintura. Inclui padding/borda, exclui margem e acompanha alinhamentos, relative/absolute, flex/grid e stretch. Descendentes que excedem o pai não aumentam indevidamente sua caixa.
- getBoundingClientRect para Element nas rotas direta, extraída e emprestada, com snapshot ordinário novo de oito campos numéricos. O objeto antigo permanece estático e é mutável pelo chamador sem alterar o DOM ou nova medição. O helper usa as quotas de objeto/propriedades existentes, sem tabela de handles ilimitada ou setters do protótipo durante criação.
- ViewContext explícito no IPC 10: Origin declarado pelo host, Sampled com scroll e clip CSS reais, ou None/Unknown. O worker não transforma contexto ausente em zero. O cache conserva coordenadas documentais; scroll isolado não consome nova resolução de layout.
- Instalação e validação do contexto antes de Scripts, eventos, timers e RAF. Desktop associa a amostra ao documento, às dimensões renderizadas e ao zoom, captura após ScrollArea, e só depois oferece timers/RAF. Mudanças de altura e largura fracionária também disparam reflow.
- NoBox retorna zeros; geometria existente com origem desconhecida ou categoria ainda não medida falha explicitamente, sem inventar retângulo. visibility:hidden mantém caixa e clipping não reduz a medição.

## Execução e auditoria

Rust 1.99.0 local, CARGO_NET_OFFLINE=true, build jobs=1, target externo à árvore. Todos os quatro gates da tentativa 4 e o runner encerraram com exit 0:

```text
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
```

Auditoria nominal independente recontou o log fechado: 1451 entradas R4 preservam nomes, resultados e sufixos; 52 entradas novas passaram. Os oráculos históricos não foram enfraquecidos. Mudanças intencionais são duas expectativas de protocolo 9→10 e inicializações de contexto/estado para as interfaces novas.

O ignored permanece tests::bundled_model_produces_real_output, que requer runtime/modelo indisponível. Inferência real não foi executada.

Cobertura nova: 17 testes de layout/provider, 12 bindings, 4 VM, 3 IPC, 3 contexto renderer, 8 desktop e 5 de integração renderer/worker. Inclui frações, coordenadas negativas, border-box sem margem, hidden/clipping, NoBox/Unsupported, validação de provider/contexto, limite de 64 resoluções, mais de 64 atualizações só de scroll, rollback, objeto mutável isolado, script inicial, callbacks, continuação de Scripts, PID e ausência de replay. O teste desktop de ScrollArea é headless, não validação da janela real/GPU.

## Tentativas anteriores preservadas

As tentativas 1–3 passaram por formatação e fmt-check, mas pararam em Clippy antes de iniciar testes. Não representam três suítes executadas nem falhas funcionais de testes.

1. collapsible_if em validate_view_context. Corrigidas duas condições aninhadas equivalentes (IPC e validação de boxes) com let-chain, sem allow, alteração de quotas ou oráculos.
2. Um literal Viewport compacto de teste legado faltava view_context. Adicionado None explicitamente, preservando HTML e verificações.
3. Um helper Layout legado de R2 faltava geometry_order e unsupported_geometry. Inicializado conforme o construtor de produção, sem mudar os oráculos de fundo.

A revisão estática inicial não detectou essas omissões de integração. O compilador/Clippy as detectou; os deltas 2+1+1 arquivos, fontes completas, logs, exits e mapas das três tentativas estão preservados. Cada nova tentativa executou os quatro gates desde o início. Não houve retry focado ou remoção de teste.

## Comparação local antes/depois

Após a suíte aprovada, exatamente dois renders da mesma fixture inline a 320×240, sem rede, ambos exit 0 e processos recolhidos. Exit 0 do CLI não significa que o script da base completou.

- R4: título R5 pending e warning Runtime: Unsupported DOM method getBoundingClientRect. Popup em (8,8), tamanho 48×20.
- R5: título R5 anchored, sem warnings. Popup em (118.5,72.25), tamanho 48×20, imediatamente após right/bottom da âncora.
- Âncora preservada em ambos: (28.5,38.25), tamanho 90×34. Os dois rasters PPM foram inspecionados visualmente e concordam com os frames JSON.
- O A/B observa somente estado inicial. Os testes renderer/worker da suíte exercitam expansão para 110×34 via RAF, retorno para 90×34 via click, snapshot anterior estático, Promise/RAF/click sem repetição, PID retido e contexto de Scripts antes do callback.

Comandos, hashes de binários/fixture, stdout/stderr, status, frames e PPMs estão na evidência local. Fonte, fixture, binários e bases foram reconferidos antes/depois.

## Limites e preservação

- Não foram validados Windows, GUI/GPU real, sites públicos, Google, WPT/Test262 completos, isolamento remoto Linux ou inferência real. Cargo offline e separação de processos não provam sandbox. Nenhuma nova autoridade de rede/arquivo foi concedida.
- Retorno é objeto ordinário de oito dados, não DOMRect completo: não implementa protótipo DOMRect, toJSON específico, getters derivados ao editar campos ou lista de fragmentos.
- Inline não substituído sem fragmentos medidos, estruturas de tabela sem caixa registrada, imagens inline decoradas cujo box model não está implementado e descendentes atômicos sem medição retornam Unsupported explícito. position:fixed e transformações permanecem fora do recorte CSS existente.
- Origem desconhecida recusa leitura de caixa real. Transições, restauração pendente e trabalho retido enfileirado antes de uma amostra atual podem apresentar Unknown; não se reutiliza origem antiga como se fosse atual.
- Continuam as quotas de 64 resoluções síncronas por sessão, DOM limitado e orçamento de leitura/propriedades/heap. Mutações ainda invalidam o cache; não se promete custo linear ou medição ilimitada.
- README, matriz e releases preservam registros históricos/de autoria anteriores aos gates. Este relatório registra resultados posteriores. SOURCE-MANIFEST.json e docs/source-snapshot-2026-11-09.md herdados não inventariam a R5 atual.
- ZIP contém fontes, relatório e manifesto, não executável Windows. Logs brutos, binários, imagens e snapshots intermediários ficam fora do ZIP, na pasta local de evidências; seus hashes estão no manifesto.
- Checkpoint/cópia congelada são locais e não protegem contra perda do ambiente. Backup externo privado aguarda autorização. Nenhum upload, push ou publicação foi realizado.

Próxima lacuna concreta: fragmentos inline continuam explicitamente não medidos por getBoundingClientRect. Nenhum bloco posterior foi implementado neste checkpoint.
