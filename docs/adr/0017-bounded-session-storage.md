# ADR 0017 — sessionStorage limitado por aba e origem

Data: 2026-11-08. Estado: implementação parcial; revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada. Sem validação funcional.

## Decisão e fronteira de dados

A shell possui TabSessionStorage por aba, com pares reais de strings em memória. O supervisor deriva a origem canônica HTTP(S) da URL final autorizada por load_document, depois dos redirects. O renderer não escolhe origem, aba, geração nem identidade de sessão. O mapa de outras origens nunca é enviado ao filho.

O protocolo passa a v5: SessionStorageSnapshot = Vec<(String, String)> conserva ordem; SessionStorageSeed { access, entries } usa SessionStorageAccess::{Unavailable, ReadWrite, QuotaExceeded}, com Unavailable como padrão seguro. A seleção do seed não modifica o mapa. Só o bucket corrente chega ao renderer, instalado depois da URL do documento e antes de run_initial. Não há nova autoridade, I/O nos crates web-*, credenciais, permissões ou conexão com a IA.

Essa é uma nova fronteira de DADOS IPC, mesmo sem autoridade adicional. validate_request valida seed nos dois extremos. accept_response só aceita o snapshot depois de validar versão, ID, resultado, página inteira, entradas e compatibilidade com o acesso concedido. O supervisor associa a resposta à sessão que iniciou a operação, não a uma origem declarada pelo filho. Unavailable e QuotaExceeded não podem transportar entradas. Duplicatas, excessos e combinações inválidas são recusados, não truncados nem parcialmente incorporados.

## Limites e alocação

Quotas: 128 pares/origem, 1 KiB/chave, 16 KiB/valor, soma de 64 KiB/origem e 16 buckets/aba. Bytes são UTF-8, com somas verificadas. A fronteira JS recusa surrogate isolado em vez de substituir conteúdo. Quotas não equivalem ao número de unidades UTF-16 da string JavaScript. O orçamento cumulativo de trabalho host é separado, além dos limites da VM.

O teto de frame JSON continua 48 MiB. Ele antecede a aceitação do frame, mas não substitui as quotas menores de storage. A desserialização JSON aloca vetores e strings antes da validação semântica de contagem, duplicatas e quotas. Não se promete validação de todos esses limites antes de qualquer alocação, zero-copy ou teto de heap igual a 64 KiB. Cópias, serialização e snapshots consomem memória adicional; permanecem limites de processo e trabalho existentes.

Origem nova com snapshot vazio não ocupa slot. Bucket existente esvaziado continua ocupando seu slot. Ao atingir 16 origens, não há eviction: origem ainda não retida recebe QuotaExceeded, permite leituras vazias e recusa setItem com erro ordinário capturável. Origens já retidas continuam disponíveis.

## Transações e vida útil

O rollback restaura State, incluindo o contador host read_work. O budget consumido preservado é o da VM; não se declara retenção do contador host após rollback. Resource desabilita scripts e execução futura, não oferece reinício gratuito de trabalho à página.

O estado do engine participa do rollback de DOM/VM do ADR 0007. Falha ordinária de quota não altera o bucket e pode ser capturada; efeitos anteriores permanecem. Esgotamento de trabalho VM/host é Resource fatal: restaura a operação sem devolver orçamento VM consumido; a falha fatal desabilita execução futura, e não pode ser suprimido por catch/finally.

Navegação inicial superada descarta suas escritas e não publica página antiga. Operação já em andamento de uma sessão comprometida, quando outra navegação é enfileirada, pode confirmar o checkpoint da origem antiga após validação integral, sem publicar sua página, antes da navegação seguinte. Isso não equivale a aceitar uma navegação inicial obsoleta.

Crash, timeout, falha de transporte ou frame inválido conservam somente o último checkpoint confirmado. Não há promessa de recuperar escritas do filho ainda não confirmadas. Refresh, A→B→A, histórico e HOME/SETTINGS conservam buckets da mesma aba, embora documento/VM sejam recarregados. Outras abas/workspaces não compartilham o mapa. Fechar a aba elimina buckets; reiniciar não os restaura. Perfil e restauração de URLs não incluem storage.

## Privacidade e recusas

file:, noir:, data:, origens opacas e URL ausente ficam indisponíveis; não existe bucket null compartilhado. Storage não é incluído diretamente pelo navegador em perfil, disco, logs ou contexto/ferramentas IA. A página pode ler seus próprios valores e inseri-los no DOM ou outro conteúdo que controla: não há garantia de sigilo contra a própria página.

Não implementa localStorage, cookies, IndexedDB, Cache Storage, workers, offline, eventos storage, named properties/enumeração ou interfaces Storage/DOMException completas. Políticas de rede, sandbox e broker permanecem.

## Evidência

Revisão textual final do item 11 concluída sem bloqueadores; checagem estática aprovada conforme registro na nota do item 11. Não foram executados testes, fixtures, aplicativo, navegação, Clippy ou build de distribuição nesta etapa documental. Evidências anteriores não validam este recorte. Itens 1–11 ABERTOS/parciais; item 19 adiado. Sem commit ou publicação.

- [Subconjunto de sessionStorage](../specifications/session-storage-subset.md)
- [Nota do item 11](../releases/2026-11-08-session-storage.md)
- [Rollback: ADR 0007](0007-script-error-semantics.md)
