# E04 — Classe com encapsulamento

Quarto degrau. Primeiro a sair de structs POD (E03) para uma classe de
verdade: métodos, `this` implícito, visibilidade, campo estático,
construtor múltiplo, método `const`.

## O que ele forçou a existir

- `ir::Expr::{This, ConstructorCall}`, `ir::Expr::Call.target` (receptor
  opcional — `None` para função livre, `Some(This)`/`Some(expr)` para
  chamada de método).
- `ir::Method`, `ir::Constructor`, `ir::Record.{static_fields, constructors,
  methods}`.
- `lower::cpp::dart_member_name` — resolve o nome do lado Dart de um membro
  (`clang_getCXXAccessSpecifier`: privado/protegido ganha `_` na frente,
  público fica como está) — usado tanto na declaração do campo quanto em
  toda referência a ele, para as duas nunca poderem divergir.
- `lower::cpp::constructor_ordinal` — computa o índice 0-based de um
  construtor entre os construtores não-cópia/movimentação da classe,
  casando por USR. Chamado tanto no próprio `lower_constructor` quanto em
  todo `Expr::ConstructorCall`, garantindo que o índice usado para nomear
  (`emit::dart::dart_constructor_name`) e o usado para chamar nunca possam
  discordar. Dart não tem sobrecarga de construtor por assinatura: o
  primeiro (índice 0) vira o construtor sem nome da classe; os demais viram
  `ClassName.ctorN`.
- `emit::dart::emit_constructor`/`emit_method` — uma classe com construtor
  próprio (`record.constructors` não-vazio) passa a emitir campos, estáticos,
  construtores e métodos de verdade, em vez do construtor posicional
  sintético do E03. As duas formas não se misturam no mesmo `Record` — ver
  comentário em `emit_record`.

## Armadilhas

- **`clang_visitChildren` não expõe `this` implícito como filho
  visitável.** `clang -Xclang -ast-dump` mostra um nó `CXXThisExpr
  "implicit this"` dentro de todo `MemberRefExpr` implícito (`saldo_` em vez
  de `this->saldo_`) — mas essa é a árvore interna completa do Clang, não a
  API de cursores do `libclang`, que simplesmente reporta **zero filhos**
  para esse mesmo cursor. Tratar "zero filhos" como erro (como fariam outros
  pontos deste módulo) quebrava todo acesso implícito a campo. Resolvido com
  `member_ref_receiver`: zero filhos vira `Expr::This` diretamente, um filho
  vira o receptor explícito lowerado normalmente. Descoberto comparando
  `ast-dump` contra o resultado real de `collect_children`, não adivinhado.

- **`libclang` sintetiza um `CompoundStmt` vazio mesmo para um construtor
  padrão totalmente implícito** (o que o E03 já dependia existir, sem
  corpo próprio nenhum). A primeira tentativa de distinguir "construtor
  padrão implícito" (que ainda deve gerar o construtor posicional sintético
  do E03) de "construtor padrão declarado pelo usuário, de corpo vazio"
  checava só "tem um filho `CompoundStmt`" — e isso é verdade nos dois
  casos, quebrando o E03 (`Ponto p;` passou a emitir `Ponto()` em vez de
  `Ponto(0, 0)`). Corrigido checando se esse `CompoundStmt` tem
  **statements dentro**, não só se existe (`constructor_has_real_body`).
  Depurado injetando `eprintln!` temporário e comparando contra o E03 antes
  de reverter.

- **Ponto flutuante literal (`10.0`) não tinha representação na IR.**
  E01–E03 nunca precisaram: todo `double` chegava por parâmetro ou por
  `Convert` a partir de um inteiro. `ContaBancaria a(10.0)` introduziu o
  primeiro `CXCursor_FloatingLiteral` (kind 107) do corpus, e caía em
  "unsupported expression cursor kind 107". Resolvido com
  `Expr::DoubleLiteral` + `evaluate_float_eval_result` (espelha
  `evaluate_int_eval_result`, usando `CXEval_Float`/
  `clang_EvalResult_getAsDouble`). `f64::to_string` do Rust imprime `10`
  para `10.0` (sem o `.0`) — mantido assim de propósito: é o mesmo padrão
  já usado para inteiro-em-contexto-double desde o E03 (Dart aceita literal
  inteiro onde espera `double`).

- **Referência não-qualificada a um campo estático (`totalContas` dentro de
  um método, sem `this.`/`ClassName.`) é `DeclRefExpr`, não
  `MemberRefExpr`.** A primeira versão só passava `dart_member_name` pelo
  caminho de `MemberRefExpr`; toda referência via `DeclRefExpr` usava a
  soletração crua do cursor. Isso não aparecia no golden (nenhum teste
  automatizado pega "identificador que não existe" — só `dart analyze`
  pega), mas gerava Dart inválido: `totalContas = totalContas + 1;`
  referenciando um identificador de topo-nível inexistente (o campo real é
  `_totalContas`, privado). Corrigido resolvendo `DeclRefExpr` pelo cursor
  **referenciado** (`clang_getCursorReferenced`) através do mesmo
  `dart_member_name`, em vez da soletração do próprio cursor — variável
  local/parâmetro continuam corretos porque não têm *access specifier*
  (`dart_member_name` devolve o nome sem alterar).

- **Corpo de construtor que só atribui um campo (`_saldo = saldoInicial;`)
  não satisfaz a análise de inicialização definitiva do Dart para campos
  não-anuláveis.** Ao contrário de variável local, `dart analyze` só
  reconhece *initializing formal* (`this.campo`) ou lista de inicializador
  (`: campo = valor`) como prova de que um campo não-anulável foi
  inicializado — atribuição dentro do **corpo** do construtor não conta,
  mesmo sendo a primeira linha e incondicional. Sem isso,
  `not_initialized_non_nullable_instance_field` derrubava `dart analyze`
  mesmo com o golden "certo". Resolvido dando a todo campo de instância de
  uma classe com construtor próprio um valor-zero na declaração
  (`double _saldo = 0;`), reaproveitando `scalar_zero_literal` (já usado
  para os estáticos) — o construtor sempre sobrescreve antes de qualquer
  leitura, então o zero nunca é observável. Só se aplica quando
  `record.constructors` não é vazio: o caminho sintético do E03 continua
  byte-idêntico ao que já era.

## Decisão de projeto tomada aqui

- **`testarContagemDeContas` usa contagem relativa, não absoluta.** O
  harness do oráculo roda todos os casos no mesmo processo, então o estado
  estático (`totalContas`) persiste entre casos — um valor absoluto
  quebraria dependendo da ordem de execução. O fixture constrói uma
  instância de referência, lê a contagem "antes", constrói mais duas, lê
  "depois", e retorna a diferença. A primeira versão do fixture deixava uma
  das duas instâncias extras (`a`) sem nenhuma leitura depois de construída
  — o que provava a existência do encapsulamento em C++, mas gerava
  `unused_local_variable` no Dart transpilado (aviso, mas `dart analyze`
  ainda assim falha para qualquer saída de `dart analyze`/`run_command`,
  aviso ou erro). Corrigido lendo `a.totalDeContas()` também (`meio`), e
  somando os dois incrementos (`(meio - antes) + (depois - meio)`) — mesmo
  resultado (2), sem variável não utilizada.
