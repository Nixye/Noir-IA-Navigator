# Roadmap: delete Member resumível limitado do item 1

Data: 2026-11-08. Vigésimo incremento do item 1. Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO.

## Referência e ordem observável

O operador delete admite await/yield na base/chave de Member ordinário. ReferenceBase/ReferenceKey capturam a referência uma vez e ReferenceOperation::Delete a consome sem ler o getter do alvo. Ordem: base, expressão-chave, rejeição nullish, ToPropertyKey, remoção. Throw/return/rejeição durante suspensão descartam a remoção pendente; finally mantém o tratamento normal e Resource permanece fatal. A expressão-chave ainda executa para base null/undefined, mas sua conversão não ocorre nesse caso; erro da expressão-chave prevalece sobre o erro nullish. A retomada não repete base nem chave.

O caminho comum de delete Member compartilha o helper. Somente o operador transforma retorno false em TypeError no modo strict; Reflect.deleteProperty conserva false. Em String/Utf16, length e índices canônicos dentro do comprimento UTF-16 são não configuráveis; astral ocupa duas unidades e lone surrogates são preservados. As chaves "01", "-0" e "1e0" são ausentes. Não há boxing temporário nem limite artificial de 127 índices. Chaves Symbol em primitivas não nullish retornam true sem lookup/getter. Wrappers e exóticos Host/Native conservam os limites existentes; isso não declara reflexão completa.

## Memória, recursos e fronteiras

O trace existente conserva base/chave/referência/continuação entre suspensões. A guarda de execução ativa conserva os temporários durante a retomada. Mantêm-se os limites compartilhados de 4096 raízes deduplicadas, 256 máquinas e 256 frames/ações/pending combinados, além dos orçamentos de heap, instruções, strings e armazenamento. Os máximos não garantem capacidade simultânea. A varredura de String para comprimento UTF-16 é cobrada; Utf16 usa seu comprimento em unidades.

Não abre delete genérico com suspensão, como delete(await value), Name suspensível, referências private/super suspensíveis, &&=/||=/??=, padrões/desestruturação, new, objetos/classes, eval direto, spread de chamada, argumentos de import, cases, labels, tagged templates, top-level await ou ECMAScript completo.

Não altera a fronteira de rede/arquivos nem incorpora outro motor.

## Entrega e evidências

Implementação parcial, com revisão textual concluída e checagem estática aprovada, sem validação funcional; item 1 ABERTO. A fixture-fonte [javascript-resumable-delete.html](../../fixtures/javascript-resumable-delete.html) prepara casos de getter não chamado, hole de array, strict/sloppy/Reflect, Symbol, strings UTF-16 e ordem/nullish em async e generator. Resultados escritos são expectativas, não observações. Erros são identificados por .name, não instanceof.

Revisão textual de Rust, documentação e fixture concluída sem achados bloqueadores. A formatação com rustfmt foi aplicada; `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked` passaram. O check terminou com código 0 em 1,95 segundo (sessão 18708), compilando os testes-fonte sem executá-los; permaneceu apenas o aviso anterior da variante `Stmt::Function` não construída. Isso é evidência estática, não validação funcional.

A fonte de integração DOM em web-bindings espera `true:0:true:false:2`. Ela não foi executada; essa string é expectativa de fonte, não resultado observado.

Nenhum teste, fixture, app/navegador, Test262, WPT, Clippy ou validação visual foi executado. Não houve build release, reconstrução de dist/Noir, empacotamento, publicação ou commit. Checks históricos não validam este incremento.

## Continuidade do roadmap

Este é o último recorte JavaScript desta sequência. Encerra o recorte atual, não o item 1: as lacunas permanecem registradas e o item 1 continua ABERTO/parcial. O avanço ao item 2 está em planejamento; ele não foi implementado nem concluído por esta entrega. Permanecem carregamento sob demanda/avaliação assíncrona de módulos, ampliação de async/generators e posições de suspensão, APIs nativas, Proxy/Reflect completos, Intl e outras lacunas de semântica/conformidade. O item 19, testes, continua adiado.

Veja [capacidades](../architecture/capabilities.md), [subconjunto JavaScript](../specifications/javascript-subset.md) e [incremento 19](2026-11-08-resumable-assignment.md).


