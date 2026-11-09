# ADR 0022: entropia e relógio explícitos por documento

Data: 2026-11-09. Estado: implementado, validação dirigida registrada na nota de release.

## Decisão

`browser-process` instala uma fonte de serviços para cada documento antes de executar scripts, tanto no worker único como na sessão retida. O backend usa `getrandom` 0.3.4 para entropia e `Instant` para tempo monotônico. `SystemTime` é consultado uma vez para fixar `timeOrigin`. Os tempos são truncados a milissegundos integrais; o relógio lógico de timers não participa.

O contrato de `web-bindings` aceita apenas três operações sem argumentos: obter u64 aleatório, origem temporal e tempo decorrido. Não recebe caminhos, URLs, sementes escolhidas pela página nem pedidos genéricos. O motor `web-*` continua sem abrir arquivos/rede ou chamar o gerador de números do sistema operacional. A fonte concreta fica em `browser-process`, fora do motor.

`Math.random` consome os 53 bits superiores e divide por 2^53: o resultado inclui zero e nunca inclui um. Não há sequência previsível de fallback nem persistência da entropia em IPC. A interface não promete ser uma API criptográfica; `crypto.getRandomValues` continua ausente.

`performance` é um objeto do runtime, idêntico a `window.performance`. `now()` exige o receptor real e retorna tempo decorrido da fonte instalada. `timeOrigin` é fixo e somente leitura. Este recorte usa propriedades próprias, sem afirmar conformidade completa com os descritores/protótipos WebIDL. APIs de eventos, marks, measures, entries e observers não são adicionadas.

## Fronteiras e falhas

A fonte pode ser instalada apenas uma vez e antes de qualquer tentativa de execução. Clones/checkpoints compartilham a mesma identidade `Arc`, portanto rollback do DOM/VM não recua o relógio nem restaura bytes já consumidos. Falha de entropia produz erro explícito, sem inventar valor; mensagens de erro não contêm diagnósticos do sistema operacional. Valores temporais não finitos ou negativos são recusados.

O orçamento normal de chamadas/instruções limita o número de consultas. A fonte concreta usa quantidade constante de memória por chamada. Nenhuma capability de rede, arquivos, perfil ou IA é concedida; AppContainer e a recusa de navegação remota Linux permanecem iguais. A disponibilidade de entropia dentro do AppContainer Windows ainda precisa de execução nessa plataforma; os testes Linux não comprovam essa fronteira.

## Evidência e limites

Testes determinísticos cobrem fronteiras de 53 bits, chamadas destacadas, aridade, argumentos ignorados, não-construção, falha, quotas, identidade, origem fixa, separação dos timers, GC e preservação da fonte em checkpoints. Um teste do backend consulta o RNG real do Linux e verifica tempo monotônico, sem teste estatístico nem promessa de imprevisibilidade certificada. Fixtures locais exercitam a integração do processo.

Esta alteração não prova Google Search funcional. A captura histórica Google não foi migrada para o cloud; seu próximo erro Host/native ainda precisa de replay autorizado sob isolamento, agora com contexto que identifica operação e alvo. Nenhuma execução remota é desviada para uma VM local sem sandbox.
