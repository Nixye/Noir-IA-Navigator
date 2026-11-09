# Snapshot de código-fonte de 2026-11-09

Este repositório contém o código-fonte atual do Noir IA Navigator, ainda experimental. A cópia de desenvolvimento no Windows foi preservada. As capacidades e lacunas estão no [checklist integral](roadmap/checklist-implementacoes-2026-11-09.md).

## Conteúdo e exclusões

Foram preservados os arquivos de código, fixtures autorais, documentação, configuração de build e licenças, incluindo alterações que ainda não tinham commit local. Não havia histórico Git local anterior a esta importação.

Não acompanham esta publicação: caches de compilação, executáveis de validação, modelos e runtime de inferência baixados, perfis, logs, capturas de sites e screenshots/conceitos em docs/assets. Esses arquivos continuam na cópia local quando existentes. Alguns relatórios e links históricos fazem referência a evidências locais que não estão neste repositório. Dois caminhos pessoais de executável foram substituídos por cargo na cópia pública do relatório de baseline.

## Estado verificado antes da migração

- Ciclo 16: workspace com 845 testes aprovados, zero falhas, um ignorado e zero filtrados; fmt e Clippy aprovados no Windows.
- Incremento posterior de propriedades do construtor Symbol: 452 testes JavaScript e 130 de bindings aprovados; fmt e Clippy dirigidos aprovados. O workspace completo ainda precisa ser repetido sobre esse último incremento.
- O replay Google continuou falhando em descritores de outro alvo Host/native, ainda não identificado. A correção de Symbol passou na reprodução sintética, mas não resolveu o site.
- Busca Google pelo GUI enviou o formulário e permitiu Voltar, mas não exibiu resultados. Foram observadas ausências de Math.random e performance. Essas duas APIs ainda não foram implementadas neste snapshot.
- O teste de inferência real permanece ignorado. Test262, WPT e compatibilidade ampla não foram comprovados.

## Continuação do desenvolvimento

Prioridade: compatibilidade e funcionamento de sites, mantendo as proteções existentes. Implementar Math.random por fonte real fornecida pelo host e performance.now/timeOrigin com relógio monotônico por documento, sem usar o relógio lógico dos timers. A proposta de integração prevê fonte injetável e testes determinísticos, backend em browser-process com getrandom 0.3.4 já presente no lock, e injeção nos dois caminhos de renderer. A dependência direta e o código produtivo ainda não foram adicionados.

O plano de [scripts dinâmicos](architecture/dynamic-script-plan.md) também é uma proposta, não implementação. O motor não deve buscar recursos diretamente.

## Windows e Linux

O toolchain está fixado em Rust 1.99.0, edição 2024. As dependências nativas Linux usadas pela CI estão em .github/workflows/ci.yml. Instalação e validação no ambiente Linux ainda precisam ser feitas.

O AppContainer é a implementação atual de isolamento remoto no Windows. No Linux, navegação remota permanece recusada até existir isolamento adequado. Testes Linux não substituem a comprovação Windows; não remover essa recusa para simular compatibilidade. O desenvolvimento dos dois sistemas deve manter os caminhos de plataforma separados e explícitos.