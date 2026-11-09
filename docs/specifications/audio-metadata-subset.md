# Metadados locais de áudio: subconjunto do item 13

Estado: implementação parcial; checagem estática do item 13 aprovada; revisão final concluída sem bloqueadores. Sem testes, execução de fixtures/aplicativo, reprodução ou validação funcional. Itens 1–13 ABERTOS/parciais; item 19 adiado.

## Elegibilidade e autoridade

Somente documento local file: recebe metadados de um elemento audio original ativo, com src relativo terminado em .wav e preload="metadata" explícito. O valor de preload é comparado sem distinguir caixa ASCII, mas sem remover espaços. Ausência, auto, none e outros valores não autorizam leitura. Template/noscript inativos não originam carregamento. A coleta acontece antes dos scripts, sem busca posterior por mutações.

O broker resolve o caminho dentro da raiz local já autorizada e aplica o confinamento existente. Caminhos absolutos, outras origens/esquemas, escapes da raiz e mídia remota não ganham acesso. Não amplia sandbox, permissões, rede ou capacidades do renderer. Não há resolução de source/srcset, seleção de codecs ou fallback de reprodução.

## Leitura e formato aceito

- RIFF little-endian com tipo WAVE, tamanho RIFF exatamente correspondente aos bytes lidos e chunks com limites verificados e padding presente quando necessário.
- Exatamente um fmt e um data, até 128 chunks no total, incluindo os conhecidos. fmt tem 16 bytes, ou 18 com cbSize zero. Outros chunks podem ser percorridos dentro dos mesmos limites, sem interpretação de seus dados.
- PCM inteiro, format tag 1, mono ou estéreo, 8/16/24/32 bits e sample rate entre 8000 e 192000 Hz. Extensible, float, RIFX, RF64 e codecs comprimidos ficam fora.
- blockAlign = canais × bits / 8; byteRate = sampleRate × blockAlign; tamanho data múltiplo de blockAlign. Operações são verificadas; duração é número de frames / sampleRate, em segundos, não um valor fictício ou estimado pelo nome do arquivo.
- Arquivo aceito até 4 MiB. A leitura pode consumir um byte sentinela adicional para detectar excesso, dentro do orçamento agregado.
- Até oito candidatos/tentativas por documento, incluindo repetidos e falhas. Até 8 MiB efetivamente lidos no agregado, incluindo arquivos inválidos e bytes sentinela. Não há reembolso ao rejeitar um arquivo.
- EOF deve ser observado. Se o agregado se esgotar sem EOF observado, a leitura é recusada conservadoramente, inclusive quando o último byte coincidir com o fim real. Não se faz leitura adicional para provar EOF depois do esgotamento.

MiB significa 1024 × 1024 bytes. Limites de tentativas, leitura, contagem e metadados são cumulativos no carregamento. EOF e tamanho declarado não provam snapshot atômico do arquivo: uma alteração concorrente ainda é risco residual. A janela preexistente entre canonicalização e abertura não foi corrigida por este incremento.

## IPC e inicialização

IPC v7 exige DocumentInput.audio_metadata; cada AudioMetadata contém src, sample_rate, channels, bits_per_sample, block_align e frames. Não há default silencioso para o campo ausente. Transporta somente esses metadados numéricos e identificadores de recurso, sem PCM, handles, descritores de arquivo ou acesso ao dispositivo. Não é transporte de áudio nem streaming. Versões anteriores são recusadas pelo versionamento normal.

O contrato recebe validação independente do parser WAV: contagem limitada a oito entradas, identificadores src até 8192 bytes cada e soma dos identificadores até 64 KiB, duplicatas e combinações inválidas recusadas, parâmetros PCM e relações aritméticas verificados. Metadados não vazios exigem documento file:. Os limites menores não substituem o frame IPC de 48 MiB nem prometem validação semântica antes de toda alocação JSON.

O seed é instalado uma única vez, antes da execução inicial, e vinculado por identidade aos nós originais elegíveis. Não há lookup de autoridade pelo src corrente. A tabela tem até oito recursos; o mapa de nós originais está sujeito ao limite DOM de 20 mil nós. A instalação do seed tem teto de 4 milhões de unidades de trabalho; o getter cobra 8192 + MAX_DEPTH + 32 unidades antes de percorrer ancestrais/comparar a fonte. Esses budgets não garantem capacidade simultânea sob os demais limites da sessão. Nós criados posteriormente e clones não herdam o mapa de metadados, ainda que tenham os mesmos atributos.

## DOM, mutações e transações

A propriedade audio.duration é somente leitura e retorna a duração real do recurso aceito ou NaN quando indisponível. Atribuir duration lança erro ordinário do subconjunto host, mesmo fora de strict; não há promessa de descritor Web IDL completo. O cálculo f64 ocorre nos bindings, não trafega uma duração de ponto flutuante no IPC. O nó também precisa continuar ativo e conectado ao documento; destacar o nó oculta o dado enquanto estiver inativo. Não cria estados de carregamento, eventos de mídia ou relógio de reprodução.

Qualquer set/remove de src invalida permanentemente os metadados daquele nó até nova sessão, inclusive escrever o mesmo valor ou restaurar o original. Alterar preload apenas aplica o gate corrente: tirar metadata oculta a duração; restaurá-lo pode torná-la visível novamente se src nunca tiver sido alterado. Não há recarga de arquivo por essas operações.

O mapa faz parte de State: Resource restaura conjuntamente DOM e metadados no rollback da operação e desabilita scripts conforme a política existente. Exceções ordinárias preservam efeitos anteriores, incluindo invalidação já realizada. Isso não devolve o orçamento de leitura do broker nem o orçamento VM consumido.

## Fora do recorte

Não há playback, play/pause/load, estados de mídia, eventos loadedmetadata/canplay, canPlayType, clock/currentTime, seek, buffering, codecs gerais, vídeo, recursos remotos, MSE, Web Audio, WebCodecs, legendas, captura, acesso a hardware ou DRM. Duração disponível não comprova compatibilidade de HTMLMediaElement ou YouTube.

## Evidência e casos pendentes

Código e documentação são fontes para revisão; não são resultados de execução. Permanecem pendentes casos de WAV válido/inválido, chunks truncados/duplicados, padding, overflow, limites por arquivo/agregado, tentativas inválidas, EOF no limite exato, esquema remoto, confinamento, clone/nó novo, src igual/restaurado, preload reversível e rollback Resource versus erro ordinário.

Veja [ADR 0018](../adr/0018-brokered-local-audio-metadata.md) e [nota da entrega](../releases/2026-11-08-wav-metadata.md).