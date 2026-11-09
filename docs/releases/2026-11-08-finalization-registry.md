# Roadmap: FinalizationRegistry limitado do item 1

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

FinalizationRegistry integra registros fracos ao coletor real, sem simular finalização por timeout ou manter o alvo vivo em uma fila forte.

## API e referências

- new FinalizationRegistry(callback), register(target, heldValue, token?) e unregister(token), com aridades, protótipo, Symbol.toStringTag e validação de receiver. Métodos não são construtores; o protótipo não tem a marca interna de um registro.
- Target e token aceitam somente Object/Array/Function vivos do heap. Symbol, Host e Native são recusados. O token pode ser o próprio alvo; heldValue não pode ser o alvo idêntico.
- Callback e held values são referências fortes apenas quando o registro é alcançável. Target e token nunca são rastreados como referências fortes. Se callback/heldValue tiver uma referência explícita ao alvo, esse caminho forte o mantém vivo corretamente.
- unregister remove todas as células do token, inclusive aquelas aguardando cleanup. Tokens mortos são limpos antes de reciclar IDs; um objeto novo não pode cancelar células de um token antigo por coincidência de endereço.
- No máximo 256 registros e 4096 células agregadas, incluindo pendências. Não há cleanupSome nem suporte a subclasses nativas/newTarget alternativo.

## Coleta e scheduling

O traçado de registros participa do ponto fixo de ephemerons de WeakMap. Só depois de a marcação terminar com sucesso o sweep limpa os alvos/tokens mortos e descarta registros inalcançáveis, antes de liberar identidades. Um registro morto libera suas células sem executar callback. O GC nunca executa JavaScript.

A pendência é representada na própria célula, sem fila global que transforme o registro em raiz forte. Coleta explícita ociosa somente identifica trabalho. Os checkpoints de execute, invoke, invoke_with_this e execute_module drenam primeiro jobs de Promise, depois trabalhos de cleanup. Cada registro é visitado no máximo uma vez por checkpoint; microtasks produzidas por ele são drenadas antes do próximo trabalho.

O contador de microtasks é compartilhado desde o drain inicial até o fim dos trabalhos de cleanup. O limite existente de 1024 jobs, o teto de 4096 callbacks e o orçamento de instruções continuam aplicáveis. Scans e deslocamentos de células são cobrados antes da mutação. GC e cleanup reentrante ficam bloqueados durante essa sequência.

A célula é removida antes da chamada do callback. Erro ordinário é reportado ao Host e interrompe o trabalho daquele registro; células restantes ficam para outro checkpoint. Resource é fatal e não permite continuar callbacks. Erros previsíveis de orçamento/entrada são verificados antes de remover a célula; uma falha já dentro da chamada não a reinsere.

## Limites e entrega

Cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram na integração. Essa checagem não comprova comportamento funcional.

Não há promessa de prazo, ordem global de finalização, execução no encerramento ou execução de todos os callbacks. Finalizadores não devem ser usados para liberar recursos essenciais ou assegurar efeitos. O scheduling descrito é uma escolha limitada do host, não conformidade completa com o event loop Web.

Testes-fonte foram preparados para marcas, GC, held values, ephemerons, tokens, reutilização de IDs, cancelamento, erros, ordem de microtasks e orçamento. A fixture fixtures/javascript-finalization.html não força GC e não promete quando a limpeza ocorrerá. Nenhum teste, fixture ou navegador foi executado.

Sem build release, alteração de dist/Noir, commit, push ou publicação. O item 1 permanece aberto.
