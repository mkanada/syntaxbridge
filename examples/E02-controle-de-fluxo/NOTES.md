# E02 — Controle de fluxo, laços, recursão

Segundo degrau. Fechado no PR4 de `docs/plans/primeiro-corte-e01-e03.md`.

## O que ele forçou a existir

- `ir::Type::{Bool, Double}`.
- `ir::BinaryOp` ganhou `Sub`, `Mul`, `Div`, `Mod`, `Lt`, `Le`, `Gt`, `Ge`,
  `Eq`, `Ne`; `ir::UnaryOp::Neg`.
- `ir::Expr::{BoolLiteral, Unary, Call}` — `Call` resolve o alvo do mesmo jeito
  que `function_catalog::record_call` (via `clang_getCursorReferenced`), e usa
  `clang_Cursor_getNumArguments`/`clang_Cursor_getArgument` para os
  argumentos — a API dedicada, que já exclui a referência ao callee, em vez de
  adivinhar por posição entre os filhos do cursor.
- `ir::Stmt::{VarDecl, Assign, If, While, For, ExprStmt}` — `lower::cpp`
  cresceu de ~110 para ~370 linhas para cobrir isso.
- No emissor, `emit_stmt`/`emit_expr` passaram a receber profundidade de
  indentação (`depth: usize`), e o mecanismo de "função inteira vira um
  throw se tiver um `Unsupported`" (do E01) virou uma busca recursiva
  (`first_unsupported_in_list`) que desce por `if`/`while`/`for` — antes só
  olhava o nível mais externo do corpo.

## Armadilhas

- **Divisão inteira trunca para `-∞`? Não — para zero, nos dois lados.**
  A armadilha documentada era "`/` vira `~/`", mas o detalhe fino (verificado
  empiricamente antes de escrever qualquer código, não presumido) é que os
  dois truncam **na mesma direção** (`-7 ~/ 2 == -3` em Dart, igual a
  `-7 / 2` em C++). Se divergissem, a correção teria que compensar o sinal;
  como não divergem, o mapeamento é `BinaryOp::Div` → `~/` quando o tipo do
  nó é `Int`, `/` caso contrário — decidido pelo **tipo do nó `Binary`**, não
  por inspecionar os operandos.

- **`ForStmt` do libclang não tem cursor "vazio" para cláusulas ausentes —
  mas isso não importou aqui.** A preocupação inicial era como
  `clang_visitChildren` se comporta quando falta `init`/`condition`/
  `increment` num `for`. Como o único `for` do fixture (`soma_ate`) tem as
  quatro cláusulas presentes, o código assume exatamente 4 filhos
  (`init, condition, increment, body`) e marca `Unsupported` com contagem de
  filhos no motivo para qualquer outra forma — decisão explícita de não
  resolver o caso geral sem um fixture que o exija (ver "nenhum caso
  especial por fixture", mas na direção oposta: não generalizar sem
  evidência também vale).

- **A cláusula de incremento do `for` (`i = i + 1`) é uma *expressão*, não um
  *statement* com wrapper próprio.** `lower_stmt` já precisava, desde antes,
  tratar "uma expressão solta como statement" (`total = total + i;` dentro do
  corpo) — reaproveitar essa mesma função para a cláusula de incremento do
  `for` funcionou de graça, sem código extra.

- **O emissor decide `~/` vs `/` pelo tipo do nó `Binary`, e isso quase
  ficou errado por um detalhe de bail-out.** A regra "função inteira vira um
  throw se tiver um `Unsupported`" (do E01) tinha alcance só no nível mais
  externo do corpo — um `Unsupported` dentro de um `if`/`while` não
  disparava o bail-out. Corrigido antes de virar bug real: a busca agora
  desce recursivamente por `then`/`else`/corpo de laço/cláusulas do `for`.

## Decisão de projeto tomada aqui (não estava fechada no plano)

- **Comparação de `double` no oráculo:** `std::setprecision(15)` no driver
  C++ gerado, para reduzir (não eliminar) a diferença entre a formatação
  padrão de 6 dígitos do `std::cout` e o `double.toString()` de Dart (que usa
  o menor decimal que ainda arredonda de volta ao mesmo bit pattern).
  Suficiente para o único caso de `double` do E02 (`7.0 / 2.0 = 3.5`), que
  imprime igual nas duas formatações. Equivalência de ponto flutuante de
  verdade (comparação por bits) é escopo de US-10/`equivalence.rs`, não deste
  harness de exemplos — registrado aqui para não ser confundido com solução
  definitiva.
