# Propriedades de objetos: quotas cumulativas da VM

O motor JavaScript próprio admite até 4.096 propriedades com chave string por
objeto ordinário, função ou armazenamento de nomes de Array. Propriedades globais
armazenadas pela VM usam o mesmo caminho. O objetivo é aceitar objetos moderados
com centenas de entradas, sem multiplicar livremente a memória pelo limite de
objetos do heap. Não acrescenta autoridade a objetos Host.

## Reservas e trabalho

- Até 65.536 reservas de propriedade no total por Runtime.
- Até 8 MiB de bytes de chaves reservados no total. Cada nome reserva quatro
  cópias persistentes: mapa de valores, ordem, descriptor e não-enumerabilidade.
- Até 4 Mi unidades de trabalho de propriedades, também debitadas de
  `Limits.instructions`. O limite padrão de instruções não muda.
- Cada chave continua limitada a 1.024 bytes.
- Descriptors de índices e `length` de Array reservam um nome separadamente,
  pois esses slots não possuem entrada no mapa de propriedades nomeadas.

São quotas cumulativas, não contadores de objetos vivos. `delete`, coleta de
lixo, exceções e transações revertidas não devolvem reservas nem trabalho.
Sobrescrever uma propriedade existente não reserva outro slot; a operação ainda
consome trabalho. Remover e reinserir a mesma chave reserva novamente.

O teto anterior de 128 nomes por objeto, combinado com 4.096 objetos, permitiria
teoricamente 524.288 nomes vivos. Agora um objeto individual pode ser maior,
mas o teto agregado de reservas é menor. As quotas não representam um limite
preciso de RSS: estruturas Rust, valores, snapshots temporários e outros
subsistemas têm custos e limites próprios. A reserva de quatro cópias cobre
as estruturas persistentes de uma chave, não todos os temporários da VM.

## Preflight e caminhos comuns

Inserções validam owner, tamanho do nome, quantidade, bytes e trabalho antes de
crescer o mapa de propriedades ou a ordem. Literais, fields, spread e descriptor
writes passam pelos mesmos helpers. JSON reserva cada chave nova antes de
inserir no builder e finaliza sem cobrar novamente. Objetos iniciais de builtins
possuem mapas pequenos já limitados, reservados antes da alocação no heap.

Ordenação usa ranks pré-computados e chaves de sort em cache, sem procurar a
posição linearmente em cada comparação. Escritas existentes não varrem a ordem.
Enumeração de nomes próprios e snapshots ordinários de `for-in` preflightam
contagens, bytes e trabalho antes de clonar as chaves. A remoção da ordem tem
varredura cobrada. Os limites próprios existentes de enumeração continuam
válidos; capacidade de armazenar 4.096 nomes não promete que toda combinação
com nomes sintéticos/protótipos caiba numa operação de enumeração.

As recusas usam `ErrorKind::Resource`. Não são capturáveis por código da página,
não executam `finally` de JavaScript nem continuam o checkpoint de jobs da
execução abortada. Não se acrescenta um estado de VM permanentemente inválida:
o embedder mantém suas regras existentes de rollback/encerramento, com o
consumo preservado.

## Restrições preservadas

Símbolos permanecem limitados a 128 propriedades por owner. Os objetos de
armazenamento de Number/Symbol, RegExp e objetos exóticos do stdlib conservam
128 nomes. Bindings por escopo, private fields, membros de classes, wrappers de
strings, typed arrays, iteradores genéricos e os limites de parser não foram
ampliados. Descriptors de hosts especiais continuam restritos. O armazenamento
numérico de Arrays possui [contrato independente](array-storage.md).

Os testes sintéticos em `property_budgets/tests.rs` cobrem compatibilidade com
centenas de entradas, fronteiras por owner/agregadas, preflight, ordem, GC,
rollback e fatalidade. A fixture local
[`javascript-property-budgets.html`](../../fixtures/javascript-property-budgets.html)
é fonte para QA do renderer; sua existência não comprova execução remota ou
compatibilidade integral com um site.
