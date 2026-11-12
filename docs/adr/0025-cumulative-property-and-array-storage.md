# ADR 0025: armazenamento de propriedades e arrays com quotas cumulativas

Status: implementado no pacote em validação.

## Contexto

Um teto pequeno por objeto rejeita dicionários comuns, mas não controla sozinho a soma de milhares de objetos. Arrays com holes também conservavam um nó de árvore por posição ausente, embora ausência seja uma informação binária. Elevar limites locais sem controlar custo agregado não é uma estratégia aceitável.

## Decisão

- Propriedades ordinárias usam reservas cumulativas por VM para slots, bytes de chaves persistentes e trabalho. O teto por owner passa a 4096 com até 65536 reservas, 8 MiB de bytes cobrados por cópias e 4 Mi unidades de trabalho, além das instruções existentes.
- Arrays usam bitmap compacto para holes, até 8192 posições por array e o mesmo teto agregado de 65536 posições. Backing values continuam densos. Métodos reservam armazenamento de resultados antes do crescimento; resultados variáveis são cobrados incrementalmente.
- Quotas cumulativas não são devolvidas por delete, GC ou rollback. O rollback ainda restaura estado observável, sem restaurar o trabalho/reservas consumidos.
- Ordens de propriedades evitam busca linear por sobrescrita e scans repetidos por comparação. Enumeração faz preflight antes de clonar conjuntos de chaves.
- Hosts, símbolos e owners exóticos conservam seus limites e contratos. Parser, instruções padrão, strings, heap e limites independentes de iteração/enumeração não são ampliados.

## Consequências

Objetos moderados e arrays de alguns milhares de posições tornam-se possíveis sem retirar limites globais. Operações caras ainda podem atingir outro orçamento. As quotas não prometem um RSS exato: valores referenciados, temporários e snapshots têm custos separados. Não muda a fronteira broker/renderer, não há I/O novo, nem alterações de IPC, sandbox ou navegação transacional.

Os contratos detalhados e testes ficam em [propriedades](../specifications/property-budgets.md) e [arrays](../specifications/array-storage.md). As fixtures são sintéticas e locais; resultados não demonstram compatibilidade completa de sites.
