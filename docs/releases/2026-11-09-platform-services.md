# Math.random, performance e diagnóstico de descritores

Implementação própria, sem motor JavaScript externo. Este incremento reduz três lacunas de diagnóstico/compatibilidade observadas no snapshot anterior; não declara Google Search resolvido.

## Entropia e tempo

- `Math.random()` agora existe, com aridade zero, descritor não enumerável configurável/escrevível, chamada destacada e resultado em [0,1). A fonte real usa getrandom no processo renderer; a VM não acessa o SO. Conversão de 53 bits é exata, e uma fonte ausente/falha produz erro em vez de valor falso.
- `performance.now()` usa `Instant` por documento e `timeOrigin` guarda um instante Unix fixo. Precisão reduzida a 1 ms, sem depender do relógio lógico de timers.
- Fonte injetável instalada antes de scripts nos dois caminhos do renderer. Fontes não são clonadas/reiniciadas em rollback. Os testes usam fontes determinísticas, não produção.
- `performance` mantém identidade com `window.performance`, chamadas verificam o receptor, e `now` não é construtor. O subconjunto usa propriedades próprias; não inclui protótipos WebIDL completos, EventTarget, PerformanceObserver, mark/measure, getEntries ou APIs de timing de rede.

O [ADR 0022](../adr/0022-document-platform-services.md) documenta contrato e fronteiras. `crypto.getRandomValues` permanece ausente.

## Descritores

Recusas de descritores Host/native conservam a mensagem e o valor lançado, mas passam a incluir contexto interno limitado com operação e alvo nativo estático/handle do host. O diagnóstico não avalia getters, não faz coerção de chaves e não copia o payload de propriedades. O contexto também aparece quando um catch falha em `_DumpException`.

O alvo restante do Google continua desconhecido: sua captura histórica não está neste checkout. Não se alargou suporte de nativos por hipótese nem se ocultou exceção para simular sucesso.

## Validação

Executado no cloud Linux, Rust 1.99, sobre o checkout compartilhado com o incremento paralelo de scripts dinâmicos:

- `cargo fmt --all --check`: aprovado.
- `cargo clippy -p web-javascript -p web-bindings -p web-renderer -p browser-process -p browser-cli --all-targets --locked --offline -- -D warnings`: aprovado.
- `cargo test -p web-javascript -p web-bindings -p web-renderer -p browser-process --locked --offline`: 461 JavaScript, 149 bindings, 15 renderer e 19 process, zero falhas/ignorados. Contagens incluem o subconjunto e as regressões existentes, não apenas este incremento.
- `cargo test -p browser-cli --test platform_services --locked --offline`: dois testes reais aprovados, um para o worker descartável e outro para a sessão retida/callback/reflow. O teste adicional cfg Windows/AppContainer foi escrito, mas não executado no Linux.
- Fixture `javascript-platform-services.html` renderizada pelo CLI em subprocesso: zero avisos e texto confirmado “Entropia, relógio e callback ativos.” Isso valida scripts e saída de dados, não inspeção visual da GUI.
- Revisão independente delimitada não encontrou defeito concreto. Lacunas de cobertura apontadas foram corrigidas: tempos inválidos, fonte de origem falha, GC sem referência externa à namespace e rollback real por esgotamento de recursos.

Validação integrada posterior, incluindo scripts dinâmicos: formatação e Clippy do workspace aprovados; 900 testes passaram, zero falhas e um teste de inferência real permaneceu ignorado por exigir modelo/runtime verificados. Windows/AppContainer, replay Google, busca real, GUI, Test262 e WPT não fazem parte desta evidência Linux. O executável anteriormente entregue não foi alterado.
