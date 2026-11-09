# Item 14: crypto.subtle.digest limitado

Estado: implementação parcial; checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores. Itens 1–14 ABERTOS/parciais; item 19 adiado. Sem validação funcional. O executável em dist/Noir não foi reconstruído; sem publicação ou nova distribuição.

## Recorte

crypto.subtle.digest oferece SHA-256/384/512 com sha2 existente e retorno Promise intrínseca de ArrayBuffer. A entrada é cópia de BufferSource real na janela da view, antes do getter algorithm.name. O método não é construtível; namespaces têm identidade estável. O cálculo local é síncrono e limitado; somente callbacks aguardam checkpoint FIFO, sem execução em background.

Erros ordinários de argumentos, conversões e getters rejeitam a Promise; receiver inválido e new digest produzem TypeError síncrono. Resource permanece fatal. NotSupportedError identifica algoritmo não suportado pelo name e não equivale a DOMException completa. Limites: 256 KiB por entrada, 2 MiB acumulados, 512 tentativas, instruções proporcionais e saída sob orçamento de ArrayBuffer. GC/rollback não devolvem acumulados.

A exposição é restrita a documento top-level HTTPS fornecido pelo supervisor, com Host negando por padrão e URL congelada antes da primeira execução. HTTP/file/noir/data/ausente não têm digest nativo. Não é algoritmo completo de secure contexts/ancestors nem concede rede ou sandbox. Veja [contrato](../specifications/crypto-digest-subset.md) e [ADR 0019](../adr/0019-bounded-crypto-https-context.md).

## Lacunas

Não implementa RNG/getRandomValues/randomUUID, chaves, encriptação, assinaturas, WebRTC, WebAuthn ou dispositivos. Não conclui Web Crypto nem o item 14. O roteiro de validação futura é textual: uma fixture file: não teria crypto pela própria política. Não foi iniciado servidor.

## Evidências

Nenhum teste, fixture, aplicativo, hash/backend, navegação, rede ou hardware foi executado nesta etapa documental. Nenhum fmt/check/build foi executado por esta etapa. Checagem estática específica do item 14 aprovada; revisão estática final concluída sem bloqueadores.

A checagem aprovada do item 13 (sessão 55686, exit 0 em 2,48 s, warning preexistente Stmt::Function) e sua revisão final sem bloqueadores são evidência histórica exclusiva do item 13; não validam este incremento nem representam validação funcional.

## Checagem estática registrada

Na integração central do item 14, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check da sessão 35131 terminou com exit 0 em 3,54 s; permaneceu somente o warning preexistente Stmt::Function. Revisão estática final concluída sem bloqueadores. Nenhum teste, hash/backend, fixture ou aplicativo foi executado. Compilação não comprova comportamento funcional.

Após acrescentar fontes de teste para sentinelas, then adulterado e orçamento de heap, a integração final repetiu cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked. O check final da sessão 77303 terminou com exit 0 em 1,45 s, somente com o warning preexistente Stmt::Function. A revisão estática final foi concluída sem bloqueadores. As fontes de teste não foram executadas; nenhum hash/backend, fixture, aplicativo ou teste foi executado. O item 14 permanece ABERTO/parcial e o item 19 adiado.
