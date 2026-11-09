# Item 17: observação textual limitada desde a extração

Data: 2026-11-08. Estado: implementação parcial; checagem estática do item 17 aprovada; revisão estática final concluída sem bloqueadores. Itens 1–16 permanecem ABERTOS/parciais; o item 17 também permanece ABERTO e o item 19, testes, continua adiado.

## Recorte

O Assistente de navegação já observa texto/display list, controles e links confirmados pela shell. Este incremento limita a cópia desde a extração até o JSON entregue ao modelo. Não introduz API completa de DOM, layout, árvore de acessibilidade ou imagem renderizada, nem WebDriver/BiDi, percepção visual ou novas ferramentas.

A shell usa composição incremental limitada, sem copiar strings inteiras para truncá-las depois. Abas ativas e estacionadas são enumeradas sob a cota antes de copiar seus títulos/URLs. Os limites textuais locais contam valores escalares Unicode (char Rust), não bytes UTF-8, grafemas ou tokens:

| Campo | Limite local |
| --- | --- |
| Título da página | 160 caracteres |
| URL da página | 2.048 caracteres |
| Texto | 4.000 caracteres |
| Seleção | 1.000 caracteres |
| Abas | 24; título de 120 e URL de 2.048 caracteres por aba |
| Elementos | 64; ID de 48, kind de 32, label/value de 160 caracteres |
| Controles | 40 |
| Rótulos de opções de select | Até oito opções habilitadas, composição limitada a 160 caracteres |

Controles password, hidden e disabled continuam excluídos. Isso não torna a observação livre de credenciais ou segredos: texto visível, títulos, URLs e campos comuns podem contê-los. Os limites não são um sanitizador geral e não tornam seguro anexar qualquer conteúdo a um provedor externo.

## Preparação e serialização para o modelo

A API é `BrowserSnapshot::model_observation(&self, allowed: [bool; 6]) -> Result<String, ObservationError>`. Recebe o snapshot por referência, sem clone integral. Antes de copiar/serializar os campos, aplica `allowed[0]` à leitura da página (title/url/text/selection/elements) e `allowed[2]` à lista de abas. Campos textuais sem leitura ficam vazios, elementos e abas não autorizados ficam sem entradas; a identidade de escopo é preservada.

A projeção do modelo usa até 2.000 caracteres de texto, oito abas, 24 elementos e 160 caracteres por URL de aba, mantendo os demais limites locais. O writer privado impõe **10.000 bytes UTF-8 durante a serialização**, incluindo escapes JSON e estrutura. Não constrói um JSON ilimitado para cortá-lo depois. Só retorna uma string JSON completa; erro não envia prefixo parcial como observação ao modelo.

O alvo de redução é 7.000 bytes, não um segundo teto absoluto. A política remove elementos do fim e depois reduz pela metade a quantidade de caracteres do texto enquanto seu comprimento em bytes ainda supera 256. As tentativas continuam limitadas; esgotadas as reduções, um JSON acima de 7.000 pode ser aceito se couber em 10.000 bytes. Falhas distinguem `ObservationError::TooLarge { max_bytes: 10000 }` e `ObservationError::Serialization`; o runtime propaga o erro antes de realizar a requisição de inferência correspondente.

O teto cobre somente o JSON da observação. Não limita o snapshot original já existente, o histórico, os prefixos do prompt, a requisição completa, tokens de contexto ou o heap/CPU global. A extração limita cópia e retenção, não estabelece um novo orçamento global de varredura da página.

## Fronteiras preservadas

- O [ADR 0006](../adr/0006-ai-browser-tools.md) recebe complemento; não há nova fronteira que exija outro ADR.
- full_access, grants, escopo aba/geração, IDs, vocabulário de ferramentas, oito/32 ações e políticas do broker permanecem.
- A geração já muda quando items, links ou title mudam. Este recorte não é uma correção de timer obsoleto e não modifica timers nem invalidação de propostas.
- Cancelar interrompe a continuação; não desfaz uma operação já iniciada.
- Dados de página continuam não confiáveis. Separação de papéis, validação estrutural e permissões não equivalem a imunidade a prompt injection.
- No provedor externo configurado, a observação autorizada pode sair do dispositivo. A limitação de tamanho não altera esse destino.

## Verificação e pendências

Checagem estática do item 17 aprovada; revisão estática final concluída sem bloqueadores. Não foram executados testes, fixtures, aplicativo, inferência ou navegação nesta etapa de documentação. Não há validação funcional, de qualidade do modelo, resistência a prompt injection ou desempenho medido. Evidência do item 17: sessão 89428, exit 0, 1,44 s. Foram executados `cargo fmt --all`, `cargo fmt --all --check` e `cargo check --workspace --all-targets --offline --locked`. Apenas o warning preexistente `Stmt::Function` permaneceu. Nenhum teste, aplicativo, inferência ou acesso à rede foi executado. Essas verificações estáticas não substituem validação funcional.

Permanecem abertos observação semântica/visual completa, esperas mais amplas, automação geral, avaliações de qualidade/segurança e integração externa. O código-fonte e a documentação não significam que o executável distribuído tenha sido reconstruído.