# ADR 0020 — Coleções e serialização limitadas no IPC

Data: 2026-11-08. Estado: implementação parcial; checagem estática do item 15 aprovada; revisão estática final concluída sem bloqueadores. Sem validação funcional.

## Contexto e decisão

O limite de frame de 48 MiB não basta para evitar expansão de vetores na desserialização nem a construção de um JSON de saída excessivo antes de verificar seu tamanho. Antecipar cotas de coleções no codec e usar um buffer privado de saída limitado reduz essas duas superfícies. O renderer continua não confiável; decodificar não equivale a aceitar a resposta.

Durante Deserialize, aplicar limites por coleção: RenderedPage.items 100.000, links 100.000, warnings 128; DisplayItem::Control.options 256; RenderResponse.session_storage e SessionStorageSeed.entries 128 pares cada. O seed de storage continua opcional onde já era opcional; options conserva o default vazio. Os campos obrigatórios continuam obrigatórios. O visitor começa vazio, não reserva pelo size_hint não confiável e só solicita crescimento até a cota. Ao alcançar N elementos, RejectExtra recusa a presença de N+1 sem desserializar esse elemento. Isso não antecipa os limites semânticos de cada valor permitido. O visitor de items também recusa mais de 256 controles, após desserializar cada item: o 257º controle pode ser construído, inclusive com até 256 options, mas não é retido. Isso é distinto da recusa de N+1 sem desserialização ao atingir a cota total da coleção.

Na saída, serde_json::to_writer serializa em um writer privado com teto MAX_MESSAGE_BYTES (48 MiB), sem crescimento solicitado acima do limite. Somente após serialização bem-sucedida seguem prefixo u32 little-endian, corpo e flush. Falha de Serialize ou excesso de bytes deixa o transporte intocado. Erro de transporte durante prefixo, corpo ou flush pode deixar envio parcial; não há promessa de rollback do transporte.

## Contrato preservado

O formato JSON, framing, versão 7 e defaults não mudam. Nenhuma cota habilita aceitação automática: continuam necessárias validação de versão/ID, resultado/página inteira, geometrias, controles, acesso e pares de storage antes do commit do checkpoint. As validações semânticas posteriores permanecem independentes das cotas do decoder. Não se altera a autoridade de arquivos, rede, credenciais, processos ou dispositivos.

## Riscos residuais e alternativas

- O frame recebido de até 48 MiB ainda é alocado por inteiro antes da decodificação. O codec não é streaming de entrada ou zero-copy.
- Strings permitidas, incluindo strings escapadas e buffers temporários do parser JSON, podem alocar antes da validação semântica menor. Barrar N+1 não limita antecipadamente o tamanho de cada um dos N elementos aceitos.
- As cotas são locais, inclusive em coleções aninhadas, não um orçamento global de heap. Capacidade solicitada limitada não promete tamanho físico exato: o alocador pode arredondar reservas.
- A string de Result::Err permanece sujeita ao teto de frame, não a uma nova cota própria menor.
- O writer limita o buffer JSON produzido, não o objeto já construído nem CPU, alocações ou varreduras internas de Serialize antes de produzir bytes. O trabalho agregado e a superfície de DoS não desaparecem.
- Serializar diretamente no transporte evitaria o buffer privado, mas poderia emitir um prefixo/corpo incompleto antes de detectar falha de serialização ou excesso. A decisão conserva o buffer para essa fronteira de falha.

Não há nova execução/prova de sandbox, SOP/CSP, credenciais, isolamento do SO ou política de rede. O [ADR 0010](0010-framed-sandbox-proof-and-host-launches.md) permanece registro separado do protocolo de prova NSP1 e de evidência histórica, não comprovação deste incremento. Auditoria, fuzzing e validação funcional continuam pendentes; o item 15 permanece ABERTO/parcial e o item 19 adiado.

- [Entrega e evidências](../releases/2026-11-08-bounded-ipc-codec.md)
