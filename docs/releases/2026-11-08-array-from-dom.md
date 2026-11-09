# Segunda rodada, item 1: Array.from e coleções DOM reais

Implementação parcial; revisão textual final concluída sem bloqueadores e checagem estática central aprovada. Nenhum teste, fixture, script JavaScript, aplicativo ou navegador foi executado para esta entrega. Não há validação funcional. Item 1 permanece ABERTO/parcial; item 19 permanece com EXECUÇÃO ADIADA a pedido.

Esta rodada faz um único recorte de JavaScript antes de avançar ao item 2. Não reabre a sequência dos 20 incrementos da [primeira rodada](2026-11-08-roadmap-first-pass.md), nem transforma seus recortes parciais em itens concluídos.

## Contrato e limites

`Array.from(items[, mapFn[, thisArg]])` converte fontes síncronas do subconjunto em Array real. O mapper recebe valor e índice, com o receptor `thisArg`; mapper inválido é recusado antes do consumo. Leitura e mapping são intercalados, sem materialização prévia de toda a fonte.

O caminho iterável tem precedência quando o método `Symbol.iterator` está disponível no lookup implementado para Object/Array/Function. Método não chamável ou falha na obtenção/iteração não autorizam fallback silencioso. O caminho array-like é usado quando o método está ausente/null, com leitura/conversão única de `length` e leitura normal dos índices. Posições ausentes tornam-se propriedades presentes com `undefined` no resultado. Arrays respeitam overrides do método, inclusive null/undefined explicitamente presentes, que desativam o fallback interno.

Strings primitivas usam o iterador interno UTF-16 por pontos de código, preservando lone surrogates, e não consultam overrides de `String.prototype[Symbol.iterator]`. Number/Boolean/BigInt primitivos também não consultam `Symbol.iterator` em seus protótipos antes do caminho array-like. Boxing de Symbol permanece recusado. O recorte não acrescenta suporte a símbolos/overrides de iteração em wrappers primitivos nem uma infraestrutura completa de iteração dos protótipos nativos.

O Host DOM atual não expõe `Symbol.iterator`. A lista estática real retornada por `document.querySelectorAll` é consumida por `length` e índices Host já existentes. Não se cria um iterador DOM artificial nem se ampliam os seletores. A lista conserva os nós da consulta, inclusive após remoção da árvore; o mapper lê os dados atuais desses nós, não uma cópia de texto/atributos. Nós adicionados depois da consulta só aparecem em nova consulta.

Chamadas com o construtor intrínseco Array e receivers não construtores comuns produzem Array. Construção com construtores alternativos Function/Host, inclusive Array vinculado por bind, é recusada explicitamente após a obtenção do método de iteração ou a conversão de length. Outros receivers Native também são recusados explicitamente, inclusive quando não construtores, porque sua classificação segura não está centralizada neste recorte. Não se declara construção genérica, subclasses, espécies ou exóticos completos. Leitura, presença da propriedade e metadados name/length estão integrados; `Array.from.length` é 1 e `new Array.from(...)` produz TypeError. Descritores e enumeração de Native continuam sujeitos às limitações gerais da VM. `Array.of`, `Array.fromAsync`, novos métodos/iteradores DOM e módulos assíncronos não fazem parte do recorte.

O resultado continua sujeito ao teto existente de 4096 posições e aos orçamentos compartilhados de instruções, slots, heap, strings e raízes. Comprimento array-like excessivo é recusado antes dos getters indexados. Não se aumentam limites. Resource permanece fatal: não é convertido em exceção ordinária capturável, não permite continuar callbacks e conserva o rollback DOM/VM da sessão com consumo preservado. Erros ordinários do mapper interrompem a conversão, mas não desfazem os efeitos anteriores já feitos no DOM. No caminho iterável, falhas ordinárias do mapper/escrita fecham o iterador, enquanto falhas de next/done/value não chamam return; Resource não inicia callbacks de fechamento. Origem, destino, mapper, thisArg e estado de iteração têm retenção temporária de GC. Não há novo serviço privilegiado, IPC, acesso a rede/arquivo ou alteração de isolamento.

## Fontes e evidências

- `crates/web-javascript/src/array_from.rs`: algoritmo e 11 casos-fonte de regressão da VM. As fontes de GC incluem coleta após retorno e pressão sobre raízes; não comprovam GC reentrante durante callbacks.
- `crates/web-bindings/src/array_from_tests.rs`: cinco casos-fonte com DocumentSession real: texto/atributos e thisArg, snapshot/identidade/mutação, lista vazia e mapper inválido, erro ordinário com efeitos anteriores e Resource com rollback/desativação.
- [Fixture HTML](../../fixtures/javascript-array-from-dom.html): consulta DOM real e resultado Array com join, preparada para execução futura.

As fontes de regressão não são evidência de aprovação. Revisão textual final concluída sem bloqueadores; checagem estática central aprovada. Foram executados centralmente `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked`; o check final após o encerramento das 11 fontes da VM terminou com exit 0 em 0,35 s. Permaneceu somente o warning preexistente de `Stmt::Function` em `parser.rs:179`. Nenhum teste, Clippy ou execução funcional foi realizado. `dist/Noir` não foi reconstruído e não houve publicação.
