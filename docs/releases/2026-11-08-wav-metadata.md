# Item 13: duração de WAV PCM local

Estado: implementação parcial; checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores. Itens 1–13 ABERTOS/parciais; item 19 adiado. Sem testes, execução de fixtures/aplicativo, reprodução, gravação ou validação funcional. O executável em dist/Noir não foi reconstruído; sem publicação ou nova distribuição.

## Recorte

O broker passa a coletar metadados reais de WAV PCM local para audio original ativo com src relativo .wav e preload=metadata explícito. O IPC v7 transporta somente os metadados, com validação independente, até o seed único antes dos scripts. audio.duration é somente leitura, real em segundos ou NaN; clones e nós novos não herdam os dados.

A nova classe de arquivo lida no broker fica restrita à raiz local já autorizada. Não amplia sandbox, permissões ou rede e não transfere PCM/handles ao renderer. O [ADR 0018](../adr/0018-brokered-local-audio-metadata.md) registra essa fronteira de confiança.

## Limites e comportamento

RIFF/WAVE little-endian PCM tag 1; mono/estéreo; 8/16/24/32 bits; 8000–192000 Hz; fmt de 16 bytes ou 18 com cbSize zero; um fmt e um data; até 128 chunks. Tamanhos, padding e relações de blockAlign/byteRate/frames são verificados.

Até oito tentativas incluindo falhas/repetidos, 4 MiB por arquivo aceito e 8 MiB de leitura agregada efetiva incluindo inválidos e sentinelas, sem reembolso. EOF observado é obrigatório: agregado esgotado sem EOF recusa mesmo o fim exato, sem leitura extra. Isso não garante snapshot atômico nem corrige a TOCTOU preexistente entre canonicalização e abertura.

IPC valida contagem, duplicatas, IDs src até 8192 bytes cada, soma dos IDs de até 64 KiB, aritmética PCM e file: exclusivamente. Mutação de src invalida permanentemente até nova sessão, inclusive valor igual/restaurado; preload é gate corrente reversível se src não mudou. Rollback Resource restaura DOM/mapa juntos; erro ordinário preserva efeitos anteriores. Veja o [contrato completo](../specifications/audio-metadata-subset.md).

## Lacunas

Não implementa reprodução, estados/eventos/canplay, relógio/seek, codecs gerais, mídia remota, vídeo, streaming, MSE, Web Audio, WebCodecs, captura, hardware ou DRM. Não conclui HTMLMediaElement nem torna YouTube compatível.

## Evidências

Fontes e documentação não são evidência de funcionamento. A checagem histórica do item 12 (sessão 85125, exit 0 em 1,00 s, somente warning preexistente Stmt::Function) não valida o item 13. Checagem estática específica aprovada; revisão final concluída sem bloqueadores; não foram executados testes, fixtures, aplicativo, navegação, áudio, fmt, check, Clippy ou build nesta etapa documental.
## Fixture-fonte, não executada

[audio-metadata.html](../../fixtures/audio-metadata.html) referencia [silence-250ms.wav](../../fixtures/audio-metadata/silence-250ms.wav) com preload=metadata explícito. O asset foi escrito como bytes determinísticos: RIFF/WAVE de 2044 bytes, cabeçalho de 44 bytes, PCM mono 8 bits a 8000 Hz e 2000 amostras 0x80. A duração esperada por aritmética é 0,25 s. O script apenas apresenta duration; não chama reprodução. Escrever esses bytes não envolveu gravação de microfone, reprodução, decodificação, execução da fixture ou validação funcional.
## Checagem estática registrada

Na integração central do item 13, cargo fmt --all, cargo fmt --all --check e cargo check --workspace --all-targets --offline --locked passaram. O check da sessão 55686 terminou com exit 0 em 2,48 s; permaneceu somente o warning preexistente Stmt::Function. A revisão estática final do item 13 foi concluída sem bloqueadores, incluindo os detalhes de fixture/literais.

Nenhum teste, fixture, aplicativo ou áudio foi executado. Checagem estática não constitui validação funcional ou comprovação de compatibilidade. O item 13 permanece ABERTO/parcial e o item 19 adiado.