# ADR 0009 — Existência de propriedades e modo do documento

Status: aceito. Data: 2026-11-03.

## Decisão

O operador JavaScript `in` precisa distinguir uma propriedade ausente de uma propriedade existente cujo valor seja undefined ou null. O contrato de dados entre VM e DOM passa a oferecer `Host::has_property`, com erro explícito como implementação padrão. Ele não deduz presença por leitura do valor e não adiciona acesso a arquivos, rede, broker ou ferramentas de IA.

O host DOM responde para propriedades e métodos efetivamente expostos pelo seu subconjunto, validando os handles e cobrando o orçamento de leitura. Existência em objetos de estilo cuja representação não permite responder corretamente continua recusada. Um resultado de presença não amplia as operações implementadas: aliases indiretos de todos os métodos DOM, descritores e protótipos nativos completos ainda não estão disponíveis.

O parser HTML também preserva metadados limitados de DOCTYPE em DocumentMode. `document.compatMode` consulta esses dados; declarações legadas não classificadas não são convertidas silenciosamente em modo standards. A classificação não ativa uma implementação completa de layout quirks. Clones e a compactação da árvore mantêm os metadados.

`document.prerendering` é falso porque o navegador não cria documentos especulativos. Não se inventa o estado de visibilidade de abas: hidden e visibilityState permanecem sem suporte enquanto não houver ligação com o estado real da interface.

## Fronteiras e verificação

As novas respostas são dados não confiáveis do documento, nunca instruções privilegiadas. A fronteira IPC, o isolamento AppContainer, os limites de entrada e a política de recursos do broker permanecem iguais. Os crates web-* continuam sem abrir URLs ou caminhos.

Testes cobrem DOCTYPE moderno, ausente, malformado e legado admitido/recusado; retenção em cópia; propriedades presentes com valor null; nomes inexistentes; handles DOM e ancestrais; consultas que esgotam o orçamento sem elevar limites. O teste integrado de compatibilidade conecta VM, DOM, CSS e pintura a uma fixture local, sem serviços externos.
