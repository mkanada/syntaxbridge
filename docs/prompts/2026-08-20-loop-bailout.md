# Fim dos bailouts e tipos opacos — loop executável

Reescrito em 2026-08-20 a partir da versão original deste arquivo, com as
decisões de execução esclarecidas pelo usuário. Este arquivo é o objetivo de
um loop autônomo de fundo (mecanismo `/goal`/`ScheduleWakeup`), não uma tarefa
de uma sessão só — o volume de causas (~14 mil ocorrências distintas na última
medição local) torna isso inevitável.

## Definições

- **Bailout**: trecho de código emitido que sinaliza que a transpilação não
  foi possível, mas ainda permite a compilação daquele artefato Dart —
  `Type::Unsupported`/`Stmt::Unsupported`/`Expr::Unsupported*`
  (`crates/server/src/emit/dart.rs`), renderizados como
  `// TODO(syntax-bridge): ...` + `throw UnimplementedError(...)`.
- **Tipo opaco**: tipo sintetizado sem ligação com o tipo C++ original,
  usado para substituir um tipo cuja transpilação não foi possível —
  `SyntaxBridgeOpaque`/`_syntaxBridgeUnsupported<T>`.
- **"Corrigir a causa"** não significa necessariamente virar Dart nativo.
  Para uma causa sem equivalente direto em Dart (`goto` com cleanup
  compartilhado, `reinterpret_cast`, união com reinterpretação de layout),
  corrigir significa modelá-la como uma fronteira/adaptador explícito e
  nomeado (ex.: uma classe bridge com contrato documentado), nunca como
  `dynamic` nem como bailout genérico silencioso — mesma régua do AGENTS.md
  para tipos sem mapeamento direto. Isso conta como resolvido mesmo que o
  Dart final não seja "puro".
- **Uma "causa", para fins de contagem de lote**: cada linha distinta em uma
  das três tabelas do diagnóstico (`unsupported_types`, agrupada por spelling
  de tipo; `unsupported_expressions`, agrupada por razão de expressão;
  `unsupported_statements`, agrupada por razão de statement), **mais** cada
  spelling distinto de tipo que hoje vira `SyntaxBridgeOpaque`/
  `_syntaxBridgeUnsupported` no emitido. Uma causa está "tratada" quando sua
  contagem cai a zero nas três tabelas e nenhuma ocorrência residual dela
  aparece como opaco no Dart emitido.

## Evidência e ferramentas já existentes

- `just verovio-diagnosis` roda a transpilação real sobre as 298 unidades de
  compilação do Verovio 6.2.0 (`crates/server/tests/verovio_6_2_0_transpile_diagnosis.rs`,
  teste `transpiling_the_real_verovio_6.2.0_project_reports_coverage`, com
  `#[ignore]`) e grava `.diagnosis/verovio-6.2.0.json` (+ `.md` e
  `.analyze.json`), incluindo a árvore completa de causas de bailout,
  agrupadas por spelling de tipo / razão de expressão / razão de statement
  com contagem de ocorrências — não é uma amostra, é o inventário completo do
  IR antes da emissão.
- `docs/plans/bailouts-verovio-6.2.0.md` já é o backlog priorizado por
  família de causa, com uma "Ordem de execução" (6 fases, da mais barata à
  mais cara) e uma "Regra de regressão" (teste mínimo com fixture antes de
  cada correção, medir antes/depois no Verovio real). **Este loop segue essa
  ordem e atualiza esse documento a cada rodada** — não redescobre prioridade
  do zero a cada iteração.
- `docs/plans/diagnostico-verovio-6.2.0.md` é o log histórico de achados já
  registrados (causa raiz, repro mínimo, commit da correção, impacto medido).
  Continuar esse mesmo formato de registro.

## Loop

1. **Baseline.** Rodar `just verovio-diagnosis`. Se os números atuais
   divergirem do que está registrado em `docs/plans/bailouts-verovio-6.2.0.md`,
   atualizar o documento com o snapshot atual antes de continuar (a
   ferramenta de pesquisa já confirmou que o snapshot commitado está
   desatualizado em relação ao estado real do código).
2. **Selecionar um lote de pelo menos 20 causas distintas.** Escolher,
   seguindo a ordem das 6 fases da "Ordem de execução" de
   `docs/plans/bailouts-verovio-6.2.0.md`, causas ainda não resolvidas até
   somar **no mínimo 20 causas distintas** (ver definição de "causa" acima),
   avançando por quantas famílias forem necessárias para atingir esse
   mínimo — não travar em uma família só porque ela sozinha tem menos de 20
   causas. Decisão do usuário: a transpilação real é lenta, não a correção
   em si — por isso o lote é dimensionado para minimizar quantas vezes
   `just verovio-diagnosis` roda, não para agrupar por afinidade temática.
   **Exceção do lote final**: se o total de causas pendentes (somando as
   três tabelas + opacos) for menor que 20, o lote é simplesmente todo o
   restante — não esperar acumular 20 causas que não existem mais. Esse é o
   sinal de que o loop está perto do fim (passo 6), não um bloqueio.
3. **TDD por causa.** Para cada uma das causas selecionadas no lote:
   escrever um teste mínimo que falhe reproduzindo a causa (fixture pequena,
   não o Verovio inteiro), implementar a correção, ver o teste passar. Nunca
   introduzir `dynamic` nem deixar `Type::Unsupported` como solução
   definitiva — sempre um destino Dart preciso, um adaptador nomeado, ou a
   fronteira explícita definida acima em "Corrigir a causa". Se uma causa se
   revelar mais cara do que parecia (ex.: exige refatoração grande), **ficar
   nela até zerar** em vez de trocar para outra família para "completar" as
   20 mais fácil — só interromper essa causa específica se ficar
   genuinamente bloqueado (ver "Quando parar e perguntar").
4. **Todas as causas do lote antes de remedir.** Não rodar
   `just verovio-diagnosis` de novo enquanto qualquer uma das causas
   selecionadas no passo 2 ainda estiver pendente — nenhum subconjunto vale,
   as 20+ precisam estar tratadas primeiro. Só então remedir e comparar com
   o baseline do passo 1: a contagem de cada causa do lote deve cair **a
   zero**, sem exceção; nenhuma causa nova, spelling vazio ou `dynamic` pode
   aparecer — isso é regressão de diagnóstico, não ruído aceitável (mesma
   regra já registrada no backlog).
5. **Registrar.** Atualizar `docs/plans/bailouts-verovio-6.2.0.md` (tabelas +
   uma entrada de "Atualização de <data>") e, se fizer sentido, acrescentar um
   achado em `docs/plans/diagnostico-verovio-6.2.0.md`, no mesmo formato já
   usado (causa raiz, repro, commit, impacto medido).
6. **Continuar.** Voltar ao passo 2, avançando pela "Ordem de execução",
   até que `.diagnosis/verovio-6.2.0.json` não reporte nenhuma causa em
   nenhuma das três tabelas (`unsupported_types`, `unsupported_expressions`,
   `unsupported_statements`) e nenhuma ocorrência de `SyntaxBridgeOpaque`/
   `_syntaxBridgeUnsupported` sobreviva no pacote Dart emitido — ou até ficar
   genuinamente bloqueado.

## Quando parar e perguntar

Só interromper o loop para pedir uma decisão do usuário quando a causa exigir
uma **decisão de produto**, não apenas engenharia — por exemplo, se surgir um
caso análogo aos já sinalizados em `crates/server/tests/mapping_solver_cases.rs`
(`c03_conditional_compilation_is_a_product_decision_not_a_type_mapping`,
`b05_string_used_as_text_and_binary_is_a_product_decision`): quando duas
soluções são tecnicamente válidas mas mutuamente exclusivas e a escolha muda o
comportamento observável do produto. Para o resto — incluindo os casos "sem
equivalente direto" da fase 6 — a diretriz já dada (fronteira nomeada
explícita) é suficiente para decidir sozinho e seguir. **Dificuldade técnica
não é motivo de parada**: uma causa cara ou trabalhosa não é uma decisão de
produto, é só mais tempo — ficar nela até zerar (passo 3). Só interromper por
dificuldade técnica se ficar genuinamente bloqueado (não apenas "é
trabalhoso"), e nesse caso registrar o bloqueio no backlog conforme o passo 6,
em vez de abandonar a causa silenciosamente ou trocar de família para
preencher a cota do lote.

## Método

Seguir o método de desenvolvimento do AGENTS.md (TDD, `just test` dentro do
Flatpak quando disponível) e as diretrizes de implementação (nunca `dynamic`,
mapeamento de tipos é o objetivo central do produto, bridge nomeada em vez de
tipo não convertível). Registrar no resumo de cada rodada o que foi corrigido,
os números antes/depois, e o que ficou para a próxima.
