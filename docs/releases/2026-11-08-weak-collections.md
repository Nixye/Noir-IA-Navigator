# Roadmap: WeakMap e WeakSet do item 1

Extensão posterior: [FinalizationRegistry limitado](2026-11-08-finalization-registry.md) acrescenta registros e cleanup real após a coleta, participando do mesmo ponto fixo de ephemerons.

Data: 2026-11-08. Estado: implementação parcial, sem validação funcional.

WeakMap e WeakSet passam a usar chaves realmente fracas. Objetos, arrays e funções do heap podem servir de chave, sem conversão ou mudança de identidade.

## API e limites

- New WeakMap/WeakSet, protótipos, constructor e Symbol.toStringTag.
- WeakMap.get/set/has/delete e WeakSet.add/has/delete, com marcas internas e métodos destacáveis. Inserção retorna a própria coleção; consulta de chave inválida devolve undefined/false, e inserção inválida lança TypeError.
- Construtores consomem iteráveis limitados e observam o adder correspondente. Falhas no processamento de entrada/adder fecham o iterador; falha ao obter o próximo item não o fecha. Resource permanece fatal, inclusive se surgir durante um fechamento iniciado por outro erro.
- Até 256 coleções e 4096 entradas agregadas por VM; até 4096 passos em um iterável de construtor. Limites de heap, instruções e GC continuam aplicáveis.
- Não há size, clear, keys ou enumeração fictícios.

## Coleta de memória

WeakSet não marca suas chaves. WeakMap usa marcação ephemeron: o valor só é alcançável pela entrada quando o mapa e a chave já são alcançáveis. A coleta alterna o rastreamento forte e novas entradas ativadas até um ponto fixo, permitindo cadeias de mapas/chaves sem transformar ciclos valor→chave em raízes fortes.

Cada inspeção entra no orçamento existente de um milhão de passos de GC. A arena e as tabelas fracas só são alteradas após a marcação terminar com sucesso. Entradas e marcas de coleções mortas são removidas antes de reutilizar IDs; WeakRef é limpo depois do mesmo ponto fixo.

Propriedades comuns anexadas à coleção continuam sendo arestas fortes, como em qualquer objeto. Os protótipos intrínsecos são raízes do GC.

## Limites de compatibilidade

Symbols, Host e Native não são aceitos como chaves porque não têm identidades coletáveis independentes nesse heap. Subclasses de construtores nativos e newTarget alternativo seguem fora do subconjunto. FinalizationRegistry continua pendente. A implementação não equivale a conformidade ECMAScript geral.

## Entrega

Cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram na integração. A revisão estática posterior não encontrou outros problemas sérios nos caminhos de ephemerons, marcas e reciclagem. Isso não substitui execução funcional.

Foram preparados testes-fonte para retenção/coleta, ciclos, cadeias invertidas, IDs reciclados, marcas, iteráveis, erros e limites. Nenhum teste ou navegador foi executado. Sem build release, atualização de dist/Noir, commit, push ou publicação. O item 1 permanece aberto.
