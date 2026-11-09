# ADR 0014: Grafo estático de módulos autorizado pelo broker

## Decisão

O broker reconhece scripts `type=module` externos e inline e usa somente o parser/resolvedor puro do motor próprio (`module_requests` e `resolve_module_specifier`) para descobrir imports estáticos. Não avalia JavaScript. O renderer continua recebendo exclusivamente `ScriptAsset { src, source }`, sem autoridade de arquivos ou rede e sem mudança no IPC. Assets de módulos usam `src = "noir-module:" + URL normalizada`; clássicos preservam o `src` do HTML. O marcador distingue autorização de módulo sem mudar o schema IPC. O renderer deve aceitar somente assets marcados no mapa de módulos e remover o prefixo ao registrar a URL. O campo src completo, incluindo marcador, não pode exceder 8192 bytes.

Clássicos e módulos compartilham até quatro tentativas de leitura, 128 KiB por fonte e 256 KiB de fontes entregues. Também há no máximo quatro assets entregues. URLs já tentadas são deduplicadas, inclusive falhas; o conjunto de módulos visitados interrompe ciclos e aceita no máximo 64 URLs. A descoberta de src clássicos também é limitada a 64 entradas. Somente módulos reais e inline iniciam a descoberta do grafo. Um clássico em cache não inicia imports. Fontes reutilizadas em ambos os modos continuam sujeitas ao limite total de entrega.

Arquivos locais de módulo exigem `.js` ou `.mjs`, canonicalização e confinamento ao diretório autorizado do documento. URLs file com host ou consulta continuam proibidas. Recursos remotos de módulo exigem a mesma origem do documento e todas as validações existentes de esquema, rede pública, DNS fixado, conteúdo misto, tamanho e prazo global de 25 segundos. Redirect que muda a URL é recusado antes de visitar o destino: o IPC atual não carrega URL final para reconstruir a base. Resposta clássica redirecionada ou de outra origem não pode ser reutilizada como módulo.

## Limitações e falhas

Extensão posterior: o [ADR 0015](0015-registered-dynamic-import.md) permite importação dinâmica apenas do registro já autorizado, sem ampliar a descoberta ou autoridade do broker descrita aqui.

Não há CORS de módulos, import maps, import dinâmico ou I/O pelo motor neste contrato. Falhas de política, parsing, leitura ou orçamento omitem o asset; o runtime deve diagnosticar dependência ausente, sem fallback de rede ou execução como clássico. Inline permanece no HTML e usa a URL do documento como referrer. O broker não promete que um grafo incompleto possa ser executado.

## Verificação

Foram acrescentados casos de teste para deduplicação/ciclos, imports inline, confinamento local, separação de clássicos/módulos marcados, orçamento de tentativas/assets, limite de descoberta e comprimento do src incluindo marcador. Não foram executados testes, build, check ou formatação automática, conforme solicitação da usuária para este recorte.

Módulos recusados não recebem marcador e não removem os assets clássicos legítimos. Mesmo quando o orçamento impede emitir um root ou descobrir dependências, aliases clássicos nunca podem satisfazer imports: o renderer não deve promover assets sem marcador a módulos.
