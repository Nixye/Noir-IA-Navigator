# ADR 0019 — Digest limitado e contexto HTTPS conservador

Data: 2026-11-08. Estado: implementação parcial; checagem estática do item 14 aprovada; revisão estática final concluída sem bloqueadores. Sem validação funcional.

## Decisão

Expor somente crypto.subtle.digest para SHA-256/384/512, com sha2 existente, em documentos top-level cuja URL HTTPS com host válido foi fornecida pelo supervisor. Host nega por padrão. DomHost lê metadados confiáveis, não propriedades JavaScript ou sourceURL. Não promover file:, noir:, data:, HTTP ou URL ausente.

Congelar a URL antes da primeira execução, incluindo módulos, eventos, timers e caminhos de falha. O congelamento é externo ao State restaurável; rollback não reabre set_document_url. Isso impede que uma sessão iniciada sem a capacidade a adquira por troca tardia de metadados.

Não se afirma implementar o algoritmo completo de secure contexts ou ancestors. O recorte não cria isSecureContext nem muda autoridade do broker, navegação remota ou isolamento. A autorização de HTTPS não confere acesso à rede para o motor.

## Recursos e semântica

Digest usa cópia de BufferSource real antes de ler e converter algorithm.name de dicionário. A conversão DOMString de identificador primitivo ocorre antes do snapshot. O cálculo local é síncrono e limitado; a Promise intrínseca entrega reações somente no checkpoint FIFO. Erros ordinários rejeitam; Resource permanece fatal. Até 256 KiB por entrada, 2 MiB acumulados e 512 tentativas, com instruções proporcionais e saída sob orçamento existente. GC e rollback não reembolsam acumulados.

## Alternativas e consequências

Expor digest universalmente seria mais simples, mas não representa a superfície restrita pretendida. Aceitar file/localhost como contextos potencialmente confiáveis exigiria política mais ampla, explicitamente adiada. Processamento em background e Web Crypto completo também ficam fora do recorte.

Não há RNG, chaves, encriptação, assinaturas, WebRTC, WebAuthn ou dispositivos. Hash não autentica origem nem substitui assinatura, TLS ou revisão de segurança. A nova API aumenta trabalho síncrono controlado por conteúdo não confiável; quotas limitam a superfície, sem comprovação funcional ou auditoria independente.

- [Contrato, limites e roteiro futuro](../specifications/crypto-digest-subset.md)
- [Nota do item 14](../releases/2026-11-08-bounded-crypto-digest.md)