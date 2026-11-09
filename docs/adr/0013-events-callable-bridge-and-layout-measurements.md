# ADR 0013 — Eventos, funções adaptadas e medidas síncronas

Status: implementado para compilação e avaliação posterior do usuário. Data: 2026-11-07.

## Contrato entre VM e bindings

O Host recebe uma ponte limitada ao Runtime original para ler propriedades de dicionários de opções de listeners. Leituras de capture/once/passive usam os getters reais e o orçamento da VM. Isso evita converter todo objeto em uma opção falsa ou ignorar suas condições. A ponte também permite verificar se callbacks são realmente chamáveis. Não expõe arquivos, rede, handles, endpoints, ferramentas de IA ou execução fora do interpretador.

Captura, fase de alvo e propagação percorrem um caminho DOM limitado. Once é removido antes da chamada; passive impede preventDefault de cancelar o evento. Valores de Event conservam identidade limitada e flags de propagação/cancelamento. A atribuição onerror em nós não fabrica notificações de recursos: o contrato atual não fornece eventos de falha de rede para esses elementos. O cancelamento nativo de links/formulários continua fora desta entrega; defaultPrevented representa o estado do evento da VM.

Function.call/apply/bind usam valores e limites próprios. Bound functions conservam o alvo, this e argumentos, e não ganham autoridade adicional. Chamadas inválidas passam a incluir contexto limitado, sem registrar a fonte inteira do script ou converter uma falha em chamada vazia.

## Medidas produzidas pelo motor

`GeometryResolver` é um serviço interno implementado no renderer. Os bindings dependem somente do contrato e dos dados; não passam a importar CSS/layout nem a abrir URLs ou caminhos. O renderer combina o DOM conectado com os mesmos assets autorizados, cascata, viewport e layout usados na pintura. O layout fornece dimensões das caixas principais depois de ajustes de altura, inclusive stretch de Flex/Grid.

clientWidth/clientHeight retiram borda e preservam padding; clientLeft/clientTop expõem a borda no subconjunto uniforme atual. Inline sem caixa cliente, nós destacados, display none e subárvores inativas produzem zero, sem inventar uma caixa. A raiz em standards e body em quirks usam as dimensões do viewport nos casos suportados. Modos de documento desconhecidos são recusados. innerWidth/innerHeight refletem o viewport fornecido pelo host, em pixels CSS do motor.

Snapshots de medidas são invalidados por mutação e troca de viewport. No máximo 64 resoluções síncronas por sessão executável; cada resolução e leitura também consome o orçamento DOM. Snapshots têm no máximo 20 mil nós e números finitos entre zero e um milhão. A contagem é reservada antes do cálculo; falhas de recurso encerram a execução de scripts conforme a política existente. O serviço só roda dentro do worker já isolado. Não há alteração de protocolo IPC, capabilities, AppContainer ou permissões de IA.

getBoundingClientRect, rolagem programática, offset/scroll metrics completos, estilos computados públicos e geometria CSS universal continuam pendentes. Coordenadas de viewport não são inventadas a partir de posições do documento sem dados de rolagem do host.

## Evidência

O usuário dispensou testes e validação nesta entrega. Não foram executados testes, Clippy, cargo check, navegação nem validação gráfica. A compilação release serve à distribuição para sua avaliação posterior. Fixtures e especificações descrevem casos e limites, sem se apresentarem como resultados aprovados.
