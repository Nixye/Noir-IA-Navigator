# ADR 0015: Importação dinâmica restrita ao registro autorizado

## Decisão

import(expr) é uma expressão própria no parser de módulos. O motor pode importar dinamicamente apenas fontes já registradas por quem possui autoridade para fornecê-las. Não há novo IPC, leitura de arquivo, requisição de rede ou descoberta de imports literais pelo broker. O contrato do [ADR 0014](0014-brokered-static-modules.md) e seus limites de origem, bytes, fontes e tentativas permanecem inalterados.

O referrer vem do escopo lexical do módulo, antes de avaliar os operandos, e não de uma base global mutável. Closures e callbacks preservam essa cadeia; os registros de módulos já enraízam seus escopos. Scripts clássicos, eval e Function não ganham import dinâmico neste recorte.

A avaliação dos argumentos e seus erros ocorre antes da Promise intrínseca. Conversão do especificador e leitura de options.with ainda são síncronas, mas suas falhas comuns rejeitam a Promise criada. Somente options ausente/undefined ou objeto com with undefined é aceito; qualquer valor definido em with, incluindo {}, é recusado explicitamente. Não se finge suporte à enumeração/validação de atributos.

Avaliação e conclusão usam continuações FIFO internas, sem chamar a entrada pública de módulo que coleta memória e drena jobs. Promessas e resultados pendentes são raízes. Cada chamada cria uma Promise distinta e reutiliza namespace, bindings e cache existentes. A resolução comum de Promise é usada, portanto export chamado then pode ser assimilado. Exaustão de recursos continua fatal.

## Limites

Isto não é carregamento sob demanda. Módulo ou dependência ausente do registro rejeita sem buscar conteúdo; uma fonte clássica não substitui um módulo autorizado. O registro continua limitado a 64 módulos, profundidade 64, 128 KiB por fonte e 512 KiB agregados. O broker mantém seu limite menor de quatro fontes externas compartilhadas.

Falhas de parsing são detectadas no registro, antes do import. Falhas de link fazem rollback e não têm o cache completo da especificação. Falhas de evaluation ficam armazenadas. Não há top-level await, avaliação assíncrona de grafos, import maps, import.meta, atributos ou namespace exótico completo. A fila limitada não declara equivalência geral com o host/event loop ECMAScript/Web.

## Validação

Implementação e revisão estática somente. Testes-fonte e fixture foram preparados, mas nenhum foi executado. Sem alteração de dist ou publicação.
