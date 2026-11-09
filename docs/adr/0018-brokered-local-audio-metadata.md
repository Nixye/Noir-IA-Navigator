# ADR 0018 — Metadados WAV locais mediados pelo broker

Data: 2026-11-08. Estado: implementação parcial; checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores. Sem validação funcional.

## Decisão

O primeiro recorte do item 13 lê WAV PCM local apenas para derivar duração. Isso introduz uma nova classe de arquivo lida pelo broker privilegiado, dentro da raiz já autorizada para o documento file:, e uma nova fronteira de dados no IPC v7. Não amplia a raiz, permissões, sandbox ou rede; não concede arquivos ou dispositivos ao renderer. O ADR registra a superfície nova sem afirmar isolamento adicional.

Somente audio original ativo com src relativo .wav e preload=metadata explícito origina tentativa. O broker lê bytes sob limites, exige EOF observado e interpreta RIFF/WAVE PCM tag 1; o filho recebe apenas metadados validados, nunca PCM ou handles. Não há dependência de decoder, reprodução ou carregamento sob demanda.

## Contenção e validação

Até oito tentativas, 4 MiB por arquivo aceito, 128 chunks e 8 MiB realmente lidos no agregado. Arquivos inválidos, repetidos e bytes sentinela também consomem orçamento, sem reembolso. Se o agregado zerar antes de observar EOF, recusa sem leitura adicional, mesmo no fim exato. São aceitos mono/estéreo, 8/16/24/32 bits, 8000–192000 Hz, fmt de 16 ou 18 bytes com extensão vazia e um único data; tamanhos, padding e aritmética são verificados.

IPC valida novamente contagem, IDs src até 8192 bytes cada, soma dos IDs até 64 KiB, duplicatas, relações PCM e restrição file:. O frame global de 48 MiB permanece; desserialização JSON ainda precede parte da validação semântica. Não se promete zero-copy ou teto total de heap igual à quota de metadados.

A ampliação de parsing acontece no broker privilegiado e exige futura validação de malformados/fuzzing. Leitura limitada com EOF observado não equivale a snapshot atômico. A corrida preexistente canonicalização/open continua risco residual; este recorte não a resolve nem autoriza acesso fora da raiz.

## Identidade e rollback

O seed único anterior aos scripts associa dados aos nós originais, sem compartilhar autorização com clones/nós novos. duration é somente leitura, real ou NaN. Mudar/remover src invalida até nova sessão, mesmo com valor igual/restaurado. preload é gate reversível sobre o dado já autorizado, sem novo I/O. State conserva o mapa para rollback conjunto com DOM em Resource; exceções ordinárias preservam efeitos anteriores. Budgets de leitura consumidos não são devolvidos.

## Alternativas e consequências

Reprodução simulada, duração constante e fingir eventos de mídia foram rejeitados por não representar capacidade real. Decodificação geral, streaming e dispositivo de áudio exigem contratos futuros, não decorrem desta API. Não há clock, seek, playback, vídeo, rede de mídia, MSE, Web Audio, WebCodecs, captura, hardware ou DRM.

Apenas uma etapa do item 13 é tratada; itens 1–13 permanecem ABERTOS/parciais e item 19 adiado. Checagem estática específica aprovada; revisão final concluída sem bloqueadores. Nenhum teste, fixture, aplicativo, áudio, build de distribuição ou publicação foi executado nesta etapa documental.

- [Contrato e limites](../specifications/audio-metadata-subset.md)
- [Nota do item 13](../releases/2026-11-08-wav-metadata.md)
- [Recursos autorizados: ADR 0008](0008-authorized-subresources.md)
- [Rollback: ADR 0007](0007-script-error-semantics.md)