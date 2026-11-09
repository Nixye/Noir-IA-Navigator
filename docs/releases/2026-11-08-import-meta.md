# Roadmap: import.meta do item 1

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

Módulos e funções originadas deles podem acessar import.meta. Cada módulo tem um objeto estável, criado sob demanda, com protótipo null e inicialmente extensível. A propriedade url é enumerável, gravável e configurável. Mutações e exclusões persistem naquele módulo; módulos distintos têm objetos distintos. O registro mantém o objeto enraizado para GC.

## Identidade e URL pública

O ID interno do registro e a URL de origem são campos diferentes. A API existente register_module(id, source) mantém origem igual ao ID; register_module_with_source_url(id, source, source_url) permite ao Host fornecer a URL real explicitamente. Ambas são normalizadas e validadas, sem conceder acesso a arquivo ou rede.

Módulos externos usam sua própria URL autorizada. Módulos inline conservam um ID sintético distinto para registro, mas import.meta.url recebe a URL real do documento, inclusive seu fragmento real. O identificador sintético do inline não é exposto como uma URL de origem inventada.

Imports e reexports estáticos, além de import dinâmico, resolvem contra a origem imutável do registro. Alterar ou apagar import.meta.url não muda essa base. Closures preservam o módulo lexical quando executadas posteriormente.

## Sintaxe e limites

O parser distingue declaração import de expressões import(...) e import.meta, inclusive em templates. Apenas meta é aceito após o ponto. Atribuição ao próprio import.meta é inválida; propriedades do objeto são comuns. O caminho new import.meta.UserConstructor é sintaticamente permitido, sujeito à construtibilidade do valor obtido.

Scripts clássicos, eval e Function continuam recusando a sintaxe. Não há import.meta.resolve, import maps, busca sob demanda, top-level await ou avaliação assíncrona de grafos. O cache de metadados não amplia autoridade do broker.

## Entrega

Cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram na integração. Testes-fonte foram preparados para identidade, GC, descritores, mutações, base imutável, fonte inline/externa e contextos inválidos. Nenhum teste, fixture ou navegador foi executado. Sem build release, atualização de dist/Noir, commit, push ou publicação. O item 1 permanece aberto.
