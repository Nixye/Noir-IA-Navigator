# Subconjunto de crypto.subtle.digest

Estado: item 14 em implementação parcial. Checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores. Sem validação funcional. Itens 1–14 ABERTOS/parciais; item 19 adiado.

## Exposição e identidade

O recorte oferece somente crypto.subtle.digest em contexto HTTPS de documento top-level informado pelo supervisor. O Host nega por padrão. DomHost::allows_subtle_crypto usa a URL confiável do documento, validada por url::Url, com esquema https e host não vazio. HTTP, file:, noir:, data: e URL ausente não recebem o digest nativo. Não há exceção para localhost HTTP.

A URL do loader é congelada antes da primeira execução: scripts, módulos, eventos e timers, inclusive caminhos de erro. set_document_url recusa alteração após esse congelamento ou após início da carga inicial. Propriedades JavaScript, sourceURL e mutações do DOM não promovem o contexto. O congelamento fica fora do estado restaurável pelo rollback.

Essa política conservadora não implementa o algoritmo completo de secure contexts, ancestors, contextos aninhados ou isSecureContext. Não autoriza navegação, rede, arquivos ou sandbox. Veja [ADR 0019](../adr/0019-bounded-crypto-https-context.md).

Quando o Host nega, crypto nativo em global/window é undefined; overrides ordinários da página não concedem autoridade. Quando permitido, os namespaces internos crypto/subtle têm identidade estável e são não extensíveis; suas propriedades e métodos são somente leitura e não configuráveis. Isso não torna o binding global/window crypto somente leitura: overrides e shadows JavaScript ordinários continuam possíveis, sem conceder o nativo quando o Host nega. digest é método nativo não construtível. A Promise e o ArrayBuffer do resultado usam os intrínsecos da VM, sem depender de construtores globais substituídos pela página.

## Algoritmos, entrada e ordem observável

SHA-256, SHA-384 e SHA-512 são calculados localmente com sha2 já presente no projeto. Os nomes são reconhecidos sem distinção de maiúsculas ASCII, sem aceitar aliases ou remover espaços. AlgorithmIdentifier admite nome convertido pelo caminho observável de string ou objeto com propriedade name convertida do mesmo modo. Não expõe APIs de dispositivos, RNG ou serviços externos; sha2 pode usar instruções da CPU internamente.

A entrada precisa ser BufferSource real: ArrayBuffer, TypedArray ou DataView do heap. A cópia usa somente a janela real de bytes da view, respeitando byteOffset e byteLength internos; objetos que imitam buffer/byteLength não são aceitos nem seus getters usados como dados. Para identificador primitivo, a conversão DOMString ocorre antes do snapshot; Symbol produz TypeError. Para dicionário, a cópia é feita antes de ler algorithm.name uma única vez: mutações do buffer em getter/coerção do algoritmo não alteram o snapshot já capturado.

O cálculo é síncrono e limitado durante a chamada; a Promise já está liquidada antes do retorno. Retornar Promise não significa processamento em background. Reações then/catch são jobs FIFO e rodam somente no checkpoint de microtasks existente, não inline no cálculo.

Erros ordinários de argumentos, conversões e getters rejeitam a Promise intrínseca, preservando o valor lançado. Receiver inválido e tentativa de new digest produzem TypeError síncrono. Algoritmos não suportados rejeitam com name igual a NotSupportedError; isso não representa DOMException completa. Falhas Resource continuam fatais e não são convertidas em rejeição recuperável.

## Orçamentos

O recorte reserva até 256 KiB por entrada, 2 MiB de bytes acumulados e 512 tentativas por VM/sessão. O trabalho consome também instruções proporcionais aos bytes. O consumo da entrada é contabilizado antes da cópia, inclusive quando o algoritmo posterior é inválido. A saída de 32, 48 ou 64 bytes usa o orçamento de ArrayBuffer existente: 1 MiB individual e 4 MiB vivos. Os contadores acumulados não são devolvidos por GC ou rollback. Esses tetos são limites do subconjunto, não promessas de throughput, paralelismo ou resistência completa a canais laterais.

## Fora do recorte

Não há SHA-1, digest incremental/streaming, RNG, getRandomValues, randomUUID, CryptoKey, geração/importação/exportação/derivação de chaves, encriptação/decriptação, assinatura/verificação, WebRTC, WebAuthn ou acesso a dispositivos. O item 14 continua aberto e não representa Web Crypto completo.

## Roteiro textual para validação futura, não executado

Uma fixture aberta por file: não recebe crypto nativo por política. O roteiro abaixo exige uma origem HTTPS previamente autorizada no fluxo normal do supervisor. Não inicia servidor nem solicita navegação nesta entrega.

1. Conferir identidade repetida de crypto/subtle, não construtibilidade de digest e Promise/ArrayBuffer intrínsecos após substituir os globais homônimos.
2. Conferir tamanhos de saída 32/48/64 e vetores SHA publicados com entradas vazia e abc; comparar resultados somente na futura execução autorizada.
3. Usar Uint8Array com sentinelas fora de uma subview e DataView com offset; verificar que somente a janela interna entra no hash.
4. Alterar bytes no getter algorithm.name e observar snapshot anterior; verificar ordem dos getters/coerções e rejeições ordinárias sem execução inline de callbacks.
5. Conferir rejeição de algoritmos desconhecidos e de imitações de BufferSource, inclusive getters falsos que não devem ser lidos.
6. Exercitar limites por entrada, agregado e tentativas, com falhas e rollback; confirmar Resource fatal e ausência de reembolso por GC.
7. Em documentos HTTP/file/noir/data e URL ausente, confirmar ausência do digest nativo; tentar alterar metadados após primeira execução, inclusive após falha inicial.
8. Conferir que não existem RNG, chaves, encriptação, assinaturas ou permissões adicionais.

Nenhum hash, backend, teste, fixture, aplicativo, navegação ou dispositivo foi executado para preparar este documento. Checagem aprovada e revisão estática final concluída sem bloqueadores não são evidência funcional.