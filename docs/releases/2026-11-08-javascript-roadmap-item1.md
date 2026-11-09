# Roadmap: primeiro incremento do item 1 — JavaScript

Data: 2026-11-08. Estado: **implementação parcial, sem validação funcional**.

Esta etapa retoma código que havia ficado incompleto no workspace e integra seus componentes ao interpretador próprio. O item 1 do documento `docs/roadmap/implementações Planejadas.md` permanece aberto. Os demais subsistemas não foram apresentados como concluídos.

## Código integrado e ampliado

- Sintaxe e execução limitadas de classes, herança, métodos, campos privados, parâmetros com defaults/rest, desestruturação, templates, spread e `for...of`.
- Validação de declarações antes dos efeitos, TDZ de parâmetros/defaults, diferenciação entre bindings `var` e léxicos e modo estrito no subconjunto. Declarações de função usam o binding mutável externo; expressões nomeadas conservam sua identidade interna.
- `eval` direto/indireto e `Function` adaptados ao AST novo. Eval estrito isola suas declarações; eval indireto e Function não herdam o modo estrito do chamador. Conclusões vazias preservam o valor anterior de eval.
- Strings UTF-16, incluindo unidades substitutas isoladas dentro da VM; comparação, concatenação, indexação, iteração, métodos selecionados, `String.fromCharCode` e `fromCodePoint`. A fronteira DOM UTF-8 continua recusando valores não representáveis, em vez de corrompê-los silenciosamente.
- Ampliação de arrays, escrita de `length`, ordem de propriedades, JSON parse/stringify e callbacks. Escritas de índices, push/pop e alterações de comprimento respeitam os descritores implementados.
- Objetos nativos em subconjuntos: Date, Map, Set, Symbol, ArrayBuffer, DataView e nove arrays tipados numéricos. Há limites de itens, buffers e trabalho; não há BigInt ou acesso direto a arquivos/rede.
- Object.prototype, protótipos/wrappers de String/Number/Boolean, `Object.assign`, `is`, `getOwnPropertySymbols`, `preventExtensions`, `seal`, `freeze` e consultas de integridade para os objetos comuns documentados. A cadeia de Array.prototype chega a Object.prototype. Nomes e símbolos respeitam não extensibilidade e descritores.
- Promises com resolução de thenables, then/catch/finally, resolve/reject, withResolvers, all/race/allSettled/any, AggregateError e fila FIFO limitada. `queueMicrotask` compartilha a fila, executada nos checkpoints de scripts/callbacks. Exceções comuns de microtasks são registradas no console do host sem cancelar jobs seguintes; exaustão de recursos continua fatal para a transação.
- Coleta mark/sweep não móvel em pontos seguros de entrada. Listeners, timers, closures, classes, Promises, objetos nativos e protótipos têm roots rastreados. Embedders podem reter/liberar valores explicitamente. Orçamentos cumulativos de trabalho, strings e posições de arrays continuam limitados; isso não equivale a memória ilimitada ou estabilidade prolongada comprovada.

## Correções de integração

Foram corrigidos erros de compilação em visibilidade no crate raiz, retorno do parser de Function, atribuição de desestruturação, empréstimos no parser de datas e cobertura de Value::Utf16. Métodos diretos de string agora chegam à implementação UTF-16, sem passar pelo fallback antigo. Igualdade de Symbol usa identidade; Map/Set preservam SameValueZero de números. Conversões implícitas de Symbol relevantes produzem erro em vez de texto ou NaN silencioso.

A superfície do broker, a sandbox e as permissões de páginas não foram ampliadas. Nenhum motor Web/JavaScript de terceiros foi adicionado.

## Limites e itens ainda pendentes

1. Módulos estáticos/dinâmicos, resolução de dependências, ciclos, bindings vivos e avaliação assíncrona não estão implementados.
2. `async`/`await`, generators e iteradores assíncronos ainda faltam. Promises não equivalem a suporte a essa sintaxe nem ao event loop completo de um navegador.
3. BigInt, Proxy/Reflect completos, WeakRef/FinalizationRegistry e Intl/ECMA-402 ainda faltam.
4. Classes/desestruturação/escopos ainda não cobrem toda a especificação: exemplos incluem `({a=1}=obj)`, resolução de privados de classes externas em classes internas, validação contextual completa de super/privados e Annex B.
5. Coerção geral de objetos, Arguments Object com aliases, protótipos/metadados de todos os objetos nativos e Web IDL continuam parciais.
6. Wrappers de String têm até 127 unidades UTF-16. Boxing de Symbol e `Object.setPrototypeOf` são recusados. Number.toString suporta base 10; toFixed mantém o intervalo de 0 a 20 dígitos deste subconjunto.
7. Integridade/descritores de Host, construtores nativos e exóticos, incluindo RegExp e arrays tipados, não são anunciados como completos. Operações sem garantia de invariantes são recusadas.
8. RegExp continua sem matching de surrogates isolados e sem semântica UTF-16/Unicode integral. `lastIndex` conserva o caminho especializado; descritores gerais de RegExp são recusados.
9. Callbacks de microtask geram diagnósticos no console; ErrorEvent/rejection tracking completos e integração de microtasks com todas as fases futuras do navegador permanecem pendentes.
10. Test262, Web Platform Tests, navegação real e estabilidade não foram executados. A busca do Google e o YouTube continuam sem compatibilidade funcional completa declarada.

## Verificação desta etapa

- `cargo check --workspace --offline --locked`: concluído com sucesso durante a integração. Checagem estática, sem execução do navegador ou de testes.
- Formatação Rust aplicada aos módulos JavaScript e bindings alterados.
- Aviso conhecido: variante legada `Stmt::Function` não construída pelo parser atual. Não foi suprimido artificialmente.
- Não executados: cargo test, Clippy, Test262, WPT, fixture, navegação e verificação gráfica. Os testes antigos ainda precisam ser reconciliados com o novo suporte antes da futura validação; por exemplo, expectativas antigas de rejeição de UTF-16 isolado não descrevem mais o valor interno da VM.

A fixture `fixtures/javascript-roadmap-item1.html` foi preparada, mas **não executada**. Ela reúne exemplos de classes, desestruturação, símbolos, arrays, JSON, wrappers, Map, buffers, eval e microtasks para a validação posterior solicitada.

Não houve build release, empacotamento, alteração de `dist/Noir`, commit, push ou publicação. O executável distribuído não inclui automaticamente estas alterações no código-fonte.

## Continuação

O [incremento seguinte](2026-11-08-static-modules.md) acrescenta módulos estáticos e carregamento de dependências por dados autorizados pelo broker. A implementação continua nas lacunas do item 1 antes de avançar aos próximos itens do roadmap; este relatório histórico não os marca como entregues.
