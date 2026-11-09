# ADR 0003 — IA local opcional e permissões por ferramenta

Status: aceito como direção arquitetural; inferência ainda não implementada. Data: 2026-11-01.

## Decisão

Não carregar modelos durante o início da aplicação. A inferência será local e executada em serviço separado, atrás de uma abstração de backend. Conteúdo de páginas será dado não confiável, sem autoridade para conceder permissões ou chamar ferramentas.

Cada ferramenta deverá declarar seu acesso: leitura da página ativa, seleção, busca em abas, navegação, preenchimento e ações com efeito externo. Autorizações deverão ter escopo, duração e identidade da ação. Envios, compras, exclusões e compartilhamentos precisarão de confirmação explícita na interface do produto.

## Critérios para a futura implementação

- A inicialização sem modelo deve funcionar integralmente.
- Nenhuma solicitação de inferência pode sair do dispositivo.
- O gateway deve negar ferramentas sem permissão.
- O modelo não pode ampliar o próprio escopo de acesso.
- Avaliações devem incluir prompt injection em páginas e revogação de permissões.

Um painel de configuração ou um tipo de ação, isoladamente, não representa inferência funcional. Não anunciar resumo, chat ou agentes antes da execução real e dos testes correspondentes.
