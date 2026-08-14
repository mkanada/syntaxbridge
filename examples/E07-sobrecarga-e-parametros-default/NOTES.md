# E07 — Sobrecarga e parâmetros default

Sétimo degrau. Primeiro em que a orquestração (`function_catalog`) consulta
`mapping::overload_options_for` de verdade — não só os testes unitários do
próprio solver (US-7 desde `docs/mapping-solver-cases.md`) — e o primeiro em
que uma decisão local (renomear uma sobrecarga) precisa reescrever código em
**outro lugar** (todo call site), não só o próprio site da decisão.

## O que ele forçou a existir

- `ir::Param.default_value: Option<Expr>` — um argumento default de C++
  (`int passo = 1`) vira parâmetro opcional posicional do Dart
  (`[int passo = 1]`), mapeamento direto, sem decisão de US-7 envolvida
  (não é sobrecarga).
- `function_catalog::apply_overload_renames` — passe de pós-processamento,
  depois que todo `ir::Function`/`ir::Method` já foi lowered com o nome
  C++ original: agrupa declarações por `(owning_class_usr, name)`, chama
  `mapping::overload_options_for` uma vez por grupo, e quando a decisão
  exige renomear (`"renomear-por-tipo"`/`"renomear-const-nao-const"`),
  renomeia a declaração **e** todo `Expr::Call` em todo o módulo cujo
  `callee_usr` bate com o grupo — resolvido por USR, nunca por nome, então
  nunca pode ficar inconsistente.
- `function_catalog::dart_overload_name` — esquema de nomeação
  determinístico: nome original + sufixo do tipo Dart de cada parâmetro,
  capitalizado (`formatarValor` + `[Int]` → `formatarValorInt`). Calculado
  do mesmo jeito para toda sobrecarga do grupo, então duas nunca colidem.

## Armadilhas

- **A armadilha documentada — renomear obriga a reescrever todos os call
  sites — apareceu exatamente como o plano previu, e é resolvida pela
  mesma disciplina "calcula uma vez, usa em todo lugar" que o E04 já usa
  para o índice ordinal de construtor múltiplo.** `apply_overload_renames`
  primeiro decide o novo nome de cada `usr` envolvido, monta um mapa
  `usr → novo nome`, e só depois varre **todo** `ir::Function.body`,
  `ir::Method.body` e `ir::Constructor.body` do módulo (inclusive dentro de
  `if`/`while`/`for`/valores-default de parâmetro) trocando `callee_name`
  onde `callee_usr` bate — uma segunda passada de verdade sobre a árvore
  inteira, não um ajuste feito no mesmo lugar onde a sobrecarga foi
  lowered (que não teria como saber, sozinho, quem mais chama aquela
  função).

- **A materialização do argumento default no *call site* quase quebrou a
  omissão dele.** `incrementar(10)` (chamando com `passo` omitido)
  continua reportando `clang_Cursor_getNumArguments == 2`, não 1 — o
  argumento omitido ainda aparece como seu próprio cursor
  (`CXCursor_UnexposedExpr`), só que com **zero filhos**, diferente de
  todo outro `UnexposedExpr` de açúcar que este módulo já desembrulha
  (que sempre tem exatamente um). Descoberto com `eprintln!` temporário
  comparando `kind`/contagem de filhos de cada argumento real contra este
  caso — não adivinhado a partir da mensagem de erro genérica
  ("wrapper cursor kind 100 did not have exactly one child"). Resolvido
  filtrando esse cursor específico **antes** de tentar `lower_expr` nele:
  o argumento correspondente simplesmente não entra na lista de
  argumentos lowered, e o call site em Dart (`incrementar(10)`) omite o
  mesmo argumento — correto sozinho, porque o parâmetro já foi emitido
  como opcional com o mesmo valor default.

- **Um parâmetro de tipo qualificado por namespace (`const std::string&`,
  já usado desde o E05) tem filhos `TypeRef`/`NamespaceRef` próprios no
  cursor do `ParmVarDecl` — sem relação nenhuma com valor default.** A
  primeira versão da leitura de `default_value` pegava "o primeiro filho
  do parâmetro" sem filtrar isso (a mesma armadilha do `TypeRef` que
  `lower_decl_stmt` já filtra para inicializador de variável local desde
  o E03, só que replicada aqui sem o mesmo cuidado) — quebrou **todo**
  parâmetro `std::string`/`std::vector` do E05 de uma vez, cada um
  ganhando um "valor default" bogus lowered a partir da própria
  referência de tipo, que por sua vez virava `_syntaxBridgeUnsupported(...)`
  numa posição de parâmetro opcional — e Dart exige que todo valor
  default seja uma expressão constante em tempo de compilação, então
  `dart analyze` rejeitava com `const_eval_method_invocation`. Pego pela
  suíte de regressão do próprio corpus (`examples_corpus_reports_status_per_manifest`
  falhando para E05, marcado `passa`) antes de virar golden — exatamente
  o que essa suíte existe para pegar. Corrigido filtrando
  `TypeRef`/`NamespaceRef`/`TemplateRef` antes de aceitar "o primeiro
  filho restante" como o valor default.

## Decisão de projeto tomada aqui

- **`"parametro-opcional"` (sobrecargas que só diferem em aridade) não é
  implementada neste degrau, de propósito.** `mapping::overload_options_for`
  já devolve essa opção quando aplicável, mas agir sobre ela significaria
  **fundir** duas entradas de IR (`Function`/`Method`) separadas em uma só
  com parâmetro opcional à direita — uma mudança de natureza diferente de
  renomear, e nenhum fixture deste corpus força isso ainda (o `passo`
  opcional de `incrementar` já cobre "parâmetro default" pela via mais
  simples: uma única declaração C++ com valor default, não duas
  sobrecargas). Se um grupo assim aparecer sem essa fusão implementada, o
  resultado não é silencioso: duas declarações Dart de mesmo nome no
  mesmo arquivo não compilam, e `dart analyze` aponta o erro na hora.
- **Valor default só é lido para parâmetro escalar/`Str`, não `Record`.**
  Um `Record` com valor default interagiria com o autoclone de
  passagem-por-valor do E03 de um jeito que nenhum fixture força a
  decidir ainda — mantido como limitação documentada, não uma lacuna
  silenciosa.
