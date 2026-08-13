# E01 — Função aritmética livre

Primeiro degrau da escada, e o primeiro Dart que o Syntax Bridge gerou. Fechado
nos PRs 1–3 de `docs/plans/primeiro-corte-e01-e03.md`.

## O que ele forçou a existir

- IR mínima (`crates/server/src/ir/`): `Module`, `Function`, `Param`,
  `Type::{Int, Void, Unsupported}`, `Stmt::{Return, Unsupported}`,
  `Expr::{IntLiteral, Ref, Binary, Unsupported}` — cada nó com `Origin`
  (arquivo/linha/coluna).
- Lowering C++ → IR (`crates/server/src/lower/cpp.rs`) como extensão do passe
  de `function_catalog` — nenhuma quarta passada `libclang`.
- Emissor IR → Dart (`crates/server/src/emit/dart.rs`), determinístico.
- Orquestração (`crates/server/src/transpile.rs`) e rota
  `POST /projects/transpile`, síncrona.
- O harness (`crates/server/tests/conversion_examples.rs`) e os três
  critérios de `conversao-guiada-por-exemplos.md` §5: golden, `dart analyze`/
  `dart format`, oráculo comportamental.

## Armadilhas

- **`int` de 32 bits vs 64 bits.** `soma(2147483647, 1)` estoura em C++
  (comportamento indefinido pela norma, mas na prática — clang, x86-64, sem
  UBSan — envolve para `-2147483648`) e não estoura em Dart (`int` nativo tem
  64 bits: o resultado é `2147483648`). A premissa "sem overflow" foi
  **declarada, não mascarada**: o caso correspondente em `oracle/cases.json`
  carrega `divergencia_conhecida: "overflow-int32"`, e o harness falha se os
  dois lados um dia passarem a concordar (o que indicaria que a premissa
  mudou e o registro está desatualizado) — nunca falha silenciosamente na
  outra direção.

- **`dart format` não é replicável à mão.** O primeiro emissor escrevia
  `throw UnimplementedError('mensagem longa...')` numa linha só para nós
  `Unsupported`. Funcionou nos testes unitários (que comparam texto), mas
  `dart format --set-exit-if-changed` reprovou porque `dart_style` quebra
  chamadas acima de 80 colunas — e mensagens com caminho absoluto + motivo
  estouram isso fácil. Tentar replicar a heurística de quebra de linha do
  `dart_style` à mão seria frágil. A correção foi arquitetural:
  `transpile::transpile` agora encana todo `.dart` emitido pelo `dart format
  --output=show` (lendo de stdin) antes de devolver o pacote — o formatador
  de verdade é a fonte da verdade, não uma imitação. `emit::dart` continua
  puro e testável sem toolchain; só `transpile.rs` (que já é a camada de
  orquestração) ganhou a dependência do binário `dart`.

- **Um nó `Unsupported` no meio do corpo quebra o resto da função, não só a
  linha dele.** Um `DeclStmt` não suportado (`int total = 0;`) virando
  `throw` fazia um `return total;` posterior referenciar uma variável nunca
  declarada em Dart — `dart analyze` acusava `undefined_identifier`, não
  apenas um aviso. A regra geral corrigida: se **qualquer** statement do
  corpo é `Unsupported`, a função inteira vira um único `throw` (motivo do
  primeiro nó não suportado), em vez de tentar preservar os statements
  vizinhos que podem depender do que não foi lowered. Sem isso, "silêncio é
  proibido" ficaria satisfeito na letra e violado no resultado (Dart que
  compila e está errado).

## Decisão de projeto tomada aqui (não estava fechada no plano)

- **Premissa de overflow:** declarar, não mascarar (ver armadilha acima) —
  decisão do §12 de `conversao-guiada-por-exemplos.md`, resolvida a favor da
  opção mais simples.
- **Formato do oráculo:** estruturado (`{"funcao", "args", "espera"}`), com
  campo adicional `divergencia_conhecida` — decisão do mesmo §12.
