# E03 — `struct` POD

Terceiro degrau. Fechado no PR5 de `docs/plans/primeiro-corte-e01-e03.md`.

## O que ele forçou a existir

- `ir::Type::Record { usr, name }` (carrega o nome, não só o `usr` — o
  emissor precisa do nome da classe Dart e não tem acesso ao `Module`
  inteiro em todo lugar que lida com um `Type`).
- `ir::Record`/`ir::Field`, `ir::Expr::{FieldAccess, RecordConstruct}`,
  `ir::Stmt::FieldAssign`.
- `lower::cpp::lower_record`, chamado de `function_catalog::visit_cursor`
  para `StructDecl`/`ClassDecl` — mesma passada, sem recurse cortado (os
  métodos inline de uma classe continuam alcançáveis pela travessia
  genérica).
- `emit::dart::emit_record` — classe Dart com campos mutáveis e construtor
  posicional (`Ponto(this.x, this.y);`), que serve dois papéis: é o
  construtor "de verdade" do tipo, e é o que `RecordConstruct` chama tanto
  para agregar quanto para clonar.
- No oráculo do harness (`conversion_examples.rs`): suporte a argumento
  agregado em `oracle/cases.json` (`{"x": 3.0, "y": 4.0}`), resolvido contra
  `ir::Record` re-extraído no próprio harness (não fica no `TranspiledPackage`,
  que só carrega texto).

## Armadilhas

- **A armadilha documentada — cópia por valor — apareceu exatamente onde o
  plano disse que apareceria.** `void mover(Ponto p)` copia em C++; Dart
  passa a referência. Resolvida por **cópia na entrada da função**: para
  todo parâmetro `Record` por valor, `lower::cpp::
  collect_params_with_clone_prelude` insere `p = Ponto(p.x, p.y);` como
  primeiro statement do corpo — antes de qualquer coisa que o usuário
  escreveu. A alternativa (cópia no *call site*) foi descartada porque exigiria
  saber, em todo lugar que uma função é *chamada*, quais dos seus parâmetros
  são por valor — informação que já está disponível de graça exatamente no
  lugar onde a função é *definida*. A regra é geral (todo parâmetro `Record`,
  não só os que algum fixture muta) — restrição 2 do §5 do plano.

- **Três surpresas do AST que só apareceram rodando de verdade** (nenhuma
  delas está documentada como "a" armadilha do degrau, mas cada uma quebrava
  a saída sem `dart analyze` reclamar alto o bastante para ser óbvio):

  1. **`Ponto` bare (sem `struct`/qualificador) resolve para
     `CXType_Elaborated`, não `CXType_Record`.** `lower_type` precisou de
     `clang_Type_getNamedType` para desembrulhar antes de checar o `kind`.
     Sem isso, todo parâmetro/campo do tipo `Ponto` virava
     `dynamic /* unsupported: Ponto */` — silencioso o bastante para passar
     em `dart analyze` (tipo `dynamic` aceita qualquer coisa) e só doeria em
     tempo de execução.

  2. **`Ponto p;` sem inicializador ainda tem um filho no cursor:** uma
     chamada implícita ao construtor padrão (`CXXConstructExpr`, que
     `libclang` expõe como `CXCursor_CallExpr` resolvendo para
     `CXCursor_Constructor`), e um `TypeRef` antes dele. Tratar "primeiro
     filho do `VarDecl`" como "o inicializador" — verdade para tipos
     primitivos em E01/E02 — é falso para tipos agregados. Descoberto com
     `clang -Xclang -ast-dump`, não adivinhado.

  3. **Passar `p` por valor para `mover(...)` e fazer `return p;` também
     envolvem `CXXConstructExpr`** (cópia e movimentação, respectivamente).
     `clang_getCursorReferenced` nessas chamadas resolve para o construtor de
     cópia/movimentação, não para uma função livre — sem tratamento
     específico, isso virava "unsupported call target cursor kind 24"
     (Constructor) bem no meio de funções que deveriam ser totalmente
     suportadas. Resolvido tratando cópia/movimentação como açúcar
     transparente: `lower_call_expr` reconhece um construtor de cópia/
     movimentação com exatamente 1 argumento e recursa direto nele, do mesmo
     jeito que já fazia para `UnexposedExpr`/`ParenExpr`.

- **`late Ponto p;` não resolve "sem inicializador" para um tipo `Record`.**
  Primeira tentativa: quando não há inicializador real, emitir
  `late Ponto p;` (mesma regra já usada para escalares desde o E02). Quebrou
  em `dart analyze` de verdade: `definitely_unassigned_late_local_variable`
  em `p.x = x;`, porque `late` adia a atribuição do *objeto inteiro*, mas
  `p.x = x` precisa **ler** `p` primeiro para achar o campo a escrever — e
  não há nenhum ponto em que `p` em si é atribuído. C++ resolve isso
  default-construindo o struct em memória (`p` já existe, só com conteúdo
  indeterminado); o equivalente real em Dart é um objeto de verdade, não uma
  variável adiada. Corrigido construindo um valor **zerado**
  (`Ponto p = Ponto(0, 0);`) a partir dos campos do tipo — reaproveitando o
  mesmo `record_fields_of` usado para a cópia na entrada da função. `late`
  continua correto (e é o que o emissor ainda faz) para tipos escalares sem
  inicializador — nenhum fixture E01–E03 exercita esse caso, mas a regra
  segue geral.

## Decisão de projeto tomada aqui (não estava fechada no plano)

- **Cópia na entrada da função**, não no *call site* — ver armadilha acima;
  a escolha ficou explicitamente aberta para quem implementasse (§7 PR5 do
  plano).
- **Literal de agregado no oráculo, forma dupla:** `Ponto{3.0, 4.0}` para
  C++ (sintaxe de *aggregate init*, válida porque `Ponto` não tem construtor
  próprio no C++ original) e `Ponto(3.0, 4.0)` para Dart (chamada ao
  construtor posicional gerado). Resolvido por nome de campo — o harness
  casa as chaves do objeto JSON com `ir::Record.fields` por nome, não por
  ordem, e emite na ordem **declarada** do struct.
- **Double "inteiro" (`7.0`) formata diferente em C++ (`7`) e Dart (`7.0`).**
  Não é uma divergência real — é só `std::cout` sem `showpoint` cortando o
  `.0`. Testado `std::setprecision` + `std::showpoint` juntos: produz
  `7.00000000000000` (15 dígitos), que também não bate com o
  *shortest-round-trip* do Dart. Resolvido no harness, não no produto: o
  texto de cada lado é normalizado (acrescenta `.0` se faltar) só quando o
  `espera` do caso foi escrito com ponto decimal no JSON (`is_f64()`) — o
  mesmo sinal que já distingue "isto é semanticamente um double" sem precisar
  consultar a assinatura da função. Equivalência de ponto flutuante de
  verdade (por bits) continua em aberto, é escopo de US-10/`equivalence.rs`.
