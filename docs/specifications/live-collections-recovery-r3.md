# Coleções DOM vivas: recovery-r3

Registro de autoria em 2026-11-12, antes da validação. Resultados posteriores
constarão no relatório externo do checkpoint, ligados às fontes testadas.
R3 continua a V13 restaurada via R1/R2; não recupera versões V14–49.

## Problema e recorte

Na base R2, cada leitura de children/childNodes alocava outro snapshot. Leituras
repetidas podiam esgotar a quota de snapshots, a identidade era instável e uma
referência guardada não acompanhava mutações. Esse diagnóstico foi estático;
o bloco não antecipa resultados de execução antes/depois.

children passa a ser HTMLCollection viva de elementos filhos diretos por raiz;
childNodes passa a ser NodeList viva de todos os nós filhos diretos. Cada par
raiz/tipo tem identidade estável. length, índices e item consultam o estado
atual, incluindo append/remove/reparent/reorder/innerHTML. Text participa de
childNodes, não de children. Coleções não copiam membros na criação.

HTMLCollection possui lookup de nomes e namedItem, por id OU name na ordem da
árvore, preservando nomes reservados da interface. NodeList não ganha nomes.
item fora de faixa retorna null; índice fora retorna undefined. Índice canônico
não vira fallback de nome; namedItem pode consultar explicitamente esse nome.
querySelectorAll permanece snapshot distinto: nem identidade estável nem
atualização viva são adicionadas a essa API. Sua interface preservada é
array-like parcial, com length/índices, sem item. Emprestar
childNodes.item.call(snapshot, ...) é recusado antes da coerção do argumento.
Essa limitação preservada não equivale a uma NodeList Web IDL completa.
Iteradores, protótipos completos, forEach e outras coleções DOM não são
prometidos por este recorte.

## Limites e fronteiras

Até 64 descritores vivos e 64 snapshots, em quotas separadas; snapshots limitados
a 4096 membros. Consultas compartilham orçamento de leitura de 2.000.000 unidades.
Mantêm-se limites DOM de 20.000 nós e profundidade128. Repetir a leitura do mesmo
par raiz/tipo não deve alocar descritores adicionais. Não há promessa de espaço
ilimitado, GC/reutilização de handles ou DOM/Web IDL completo.

As coleções refletem filhos estruturais reais, não visibilidade CSS nem
eligibilidade de execução. Isso não amplia o modelo de templates/inércia da
base. Não introduz arquivo/rede, segurança, IPC, dependências ou produção nova
nos crates renderer/CLI; ali há somente integração de teste.

## Cenário e oráculos escritos

A fixture live-collections-recovery-r3.html guarda children, childNodes e um
querySelectorAll antes de editar uma lista. Executa 96 pares de leituras sem
short-circuit antes de validar identidade; assim a comparação com a base não
depende de namedItem ou de um método novo incidental. Promise acrescenta linha;
RAF remove, move para arquivo e substitui innerHTML; click edita a lista de novo.
Referências guardadas devem acompanhar o estado, enquanto snapshot preserva
os nós originais, inclusive fora da raiz original.

Renderer/worker verificam textos presentes e ausentes e caixa de cada linha
pela geometria/cor, sem pressupor quantidade global de primitivas de pintura.
Worker mantém PID, jobs/RAF/click e reflow sem replay. Outro teste renderer
verifica lookup de nomes após rename/reparent, distinção Text/elementos e
snapshot retido. Entrada sintética inline usa noir:, sem autoridade HTTP/file.

Na conclusão da autoria esses testes ainda não foram executados. Nenhuma
alegação de GUI, Windows, sites públicos, Google, WPT ou isolamento validado.
