# ADR 0001 — Motor independente e contratos de dados

Status: aceito para a fundação inicial. Data: 2026-11-01.

## Contexto

O produto precisa manter o controle sobre DOM, CSS, layout, pintura e, futuramente, JavaScript. Incorporar um motor de navegador existente impediria esse objetivo. Implementar todos os padrões simultaneamente impediria uma validação útil.

## Decisão

Implementar o motor em crates Rust separados. O renderer recebe `DocumentInput` e `Viewport`, produz `RenderedPage` e não usa o filesystem nem protocolos de rede. O contrato reside em `browser-ipc`, com versão explícita e mensagens limitadas. A interface nativa consome a display list e trata entrada do usuário; ela não determina a cascata CSS nem o layout do documento.

Usar eframe/egui para janela e interface nativa. Usar bibliotecas independentes de decodificação de imagens e análise de URLs no broker. Essas bibliotecas não fornecem DOM, CSS ou layout Web.

## Consequências

O subconjunto implementado pode ser testado sem uma janela ou GPU. O CLI e o desktop utilizam o mesmo motor. A compatibilidade precisa crescer por recursos e fixtures verificáveis; a implementação inicial não herda conformidade de outro navegador. A medição de texto e os recursos de composição terão de evoluir para atingir fidelidade tipográfica completa.
