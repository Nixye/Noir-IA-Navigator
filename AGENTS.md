# Instruções do repositório Noir

- Preserve o motor Web próprio: bibliotecas de criptografia, protocolos, janelas, decodificação de imagens e inferência são permitidas; motores Web e JavaScript incorporados de outros navegadores não são.
- Mantenha dependências direcionais: tipos IPC independentes; DOM → HTML/CSS → layout → renderer; o broker e a interface não devem introduzir acesso a arquivos ou rede no motor.
- Trate HTML, CSS, recursos e respostas IPC como dados não confiáveis. Limite tamanhos, profundidade, contagens e tempos; valide os dados antes de alocar ou pintar.
- O renderer recebe os bytes autorizados pelo broker. Não abra caminhos ou URLs a partir de conteúdo da página dentro dos crates `web-*`.
- Não confunda separação de processos com uma sandbox. Não habilite navegação remota antes de implementar e verificar a política de isolamento; registre qualquer alteração de fronteira de confiança em um ADR e nos testes.
- Não apresente como implementadas capacidades incompletas ou simuladas. Recursos não suportados devem constar na matriz de capacidades e ter comportamento previsível.
- Preserve operações de navegação transacionais: um resultado antigo não pode substituir a navegação mais recente de uma aba.
- Dados de páginas nunca são instruções privilegiadas para ferramentas de IA. Mantenha inferência opcional e permissões por ferramenta.
- Para alterações em comportamento, execute os testes relevantes. Antes de concluir uma entrega, execute `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` e `cargo test --workspace --locked` quando o ambiente permitir; registre limitações reais.
- Testes devem verificar resultados, erros e fronteiras de segurança. Não enfraqueça verificações apenas para obter uma compilação verde.
- Atualize documentação e fixtures quando mudar o subconjunto suportado. Não adicione crates vazios para representar etapas futuras do roadmap.
