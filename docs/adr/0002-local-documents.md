# ADR 0002 — Documentos locais antes da navegação remota

Status: aceito para a entrega inicial. Data: 2026-11-01.

## Contexto

O renderer executa código próprio que ainda não recebeu auditoria independente. Separar o processo reduz acoplamento e permite timeout e encerramento, mas não impede o acesso ao sistema operacional após uma exploração.

## Decisão

Aceitar somente caminhos locais, URLs `file:` locais e as páginas internas reconhecidas. Negar protocolos remotos e hosts de arquivos de rede. O broker lê o documento com limite de tamanho, descobre imagens e autoriza recursos relativos contidos no diretório canônico do documento. Ele envia ao renderer apenas os dados aceitos.

Manter os recursos remotos e scripts desativados enquanto não houver uma implementação de isolamento apropriada para cada plataforma, testes de evasão e revisão independente. Não oferecer uma opção oculta que transforme esse build em navegador remoto.

## Consequências

É possível validar DOM, CSS, layout, imagens e navegação com arquivos reproduzíveis. A versão não atende ao uso diário na internet. A futura rede deverá residir em um serviço privilegiado com política de origem e permissões, sem introduzir acesso arbitrário no renderer.
