# E05 — `std::string`, `std::vector`

Quinto degrau. Primeiro que sai de tipos definidos pelo próprio projeto
(`struct`/`class` do usuário, E03/E04) para um **adaptador de biblioteca
padrão**: `std::string` → `String`, `std::vector<T>` → `List<T>`, sem que o
usuário tenha escrito nenhuma dessas classes.

## O que ele forçou a existir

- `ir::Type::{Str, List}` — não são `Type::Record`: nunca passam por
  `lower_record` (os campos de `std::basic_string`/`std::vector` são
  internos da libstdc++, não algo que uma classe Dart deveria expor).
- `ir::Expr::{StringLiteral, Index, StringByteLength}`.
- `lower::cpp::stdlib_template_name` — reconhece uma especialização de
  template como `std::basic_string`/`std::vector` pelo nome do template
  primário e pelo namespace que o envolve (não pela soletração do tipo, que
  é ambígua/inconsistente entre `libclang` e o próprio projeto do usuário).
- `lower::cpp::lower_stdlib_method_call`/`lower_stdlib_operator_call` — uma
  segunda via de resolução de chamada, paralela a `lower_method_call`
  (E04): reconhecida pelo dono do método/operador ser um tipo de biblioteca,
  não pela sintaxe da chamada.
- `emit::dart`: `String`/`List<T>` na assinatura, `utf8.encode(...).length`
  como ponte para `.size()` de string (com `import 'dart:convert';`
  inserido só quando o arquivo usa isso — mesmo padrão opt-in que o helper
  de `Unsupported` já usava), `target[index]` nativo para `operator[]` de
  vetor, `+`/`==` nativos para concatenação/comparação de string.

## Armadilhas

- **A armadilha documentada — `std::string` é bytes, `String` é UTF-16 —
  apareceu exatamente como o plano previu, e foi corrigida com ponte de
  código, não declarada como divergência conhecida.** `std::string::size()`
  conta bytes UTF-8; `Dart String.length` conta *code units* UTF-16 — os
  dois discordam para qualquer conteúdo fora de ASCII (`"ação"`: 6 bytes em
  UTF-8, 4 code units em UTF-16 — caso `oracle/cases.json` inclui
  exatamente esse texto para provar a divergência real e a correção juntas,
  não só o caso feliz ASCII). Seguindo a diretriz do `AGENTS.md` ("gere
  código ponte... em vez de declarar o tipo não convertível"), `.size()`/
  `.length()` de `std::string` não vira `.length` do Dart — vira
  `Expr::StringByteLength`, emitido como `utf8.encode(x).length`
  (`dart:convert`), que conta bytes UTF-8 igual ao C++. `std::vector::size()`
  não tem esse problema (contagem de elemento, não de codificação) — vira
  `FieldAccess` comum para `.length`, sem ponte.

- **`libclang` reporta `basic_string<char>`/`vector<int>` como
  `CXType_Unexposed`, nunca como `CXType_Record`** — descoberto com um
  `eprintln!` temporário (não com `ast-dump`, que mostra a árvore interna
  completa do Clang e não captura essa particularidade da API de tipos do
  `libclang`), depois de `Unsupported("basic_string<char>")` aparecer no
  golden abençoado mesmo com `stdlib_template_name` implementado e
  correto. `clang_getTypeDeclaration` continua resolvendo para o cursor
  real da especialização mesmo quando `.kind` é `Unexposed` — corrigido
  fazendo `CXType_Record` e `CXType_Unexposed` caírem no mesmo ramo de
  `lower_type`.

- **`std::string` é um `typedef` de `basic_string<char, ...>`, e um
  `const std::string&` de parâmetro chega como `LValueReference` →
  `Elaborated` → `Typedef` → (depois do fix acima) `Unexposed`.** Três
  desembrulhamentos, não um. Faltava `CXType_Typedef` (usando
  `clang_getTypedefDeclUnderlyingType`) e `CXType_LValueReference` (usando
  `clang_getPointeeType`) em `lower_type` — nenhum dos dois havia sido
  necessário até aqui porque nenhum fixture E01–E04 passava um parâmetro
  por referência. O fixture usa `const T&` para *todos* os parâmetros
  `std::string`/`std::vector<int>` de propósito, para não reabrir a
  armadilha de cópia-por-valor que o E03 já resolveu para `Record`
  (`examples/E03-struct-pod/NOTES.md`) — uma referência `const` nunca copia
  em C++, então não há nada nesse fixture que force a mesma decisão para
  `Str`/`List`; permanece em aberto para quando algum degrau futuro passar
  um desses tipos por valor.

- **`basic_string` mora em `namespace std { inline namespace __cxx11 {
  ... } }` na `libstdc++` (confirmado ao vivo, não documentação) — checar só
  o pai semântico imediato do template primário nunca acha `"std"`, sempre
  acha `"__cxx11"`.** `stdlib_template_name` precisou subir a cadeia de
  namespaces até achar `"std"` (ou desistir na raiz da unit de tradução), em
  vez de comparar só o primeiro nível. `vector` não tem esse namespace
  inline — a mesma função lida com os dois porque nada garante que um tipo
  de biblioteca futuro tenha ou não um.

- **`operator[]` de `std::vector` não tem a forma de uma chamada de método
  normal.** `.size()` (chamada com `.`) tem como primeiro filho um
  `MemberRefExpr`, igual a todo método do E04 — mas `valores[i]` (sintaxe de
  operador) tem como filhos `[receptor, referência-à-função-operador,
  índice]`, três cursores soltos, nenhum `MemberRefExpr`. Descoberto
  empiricamente (`eprintln!` dos kinds reais), depois que a checagem
  compartilhada "primeiro filho tem que ser `MemberRefExpr`" rejeitava toda
  indexação de vetor. `lower_stdlib_method_call` trata `operator[]` num
  ramo totalmente separado, antes de sequer olhar para a forma
  `MemberRefExpr`.

- **O tipo de retorno de `operator[]` (`const_reference`, um alias
  dependente de template) não resolve para nada usável.** Mesmo depois do
  fix de `CXType_Unexposed`, `lower_type(clang_getCursorType(call_cursor))`
  para `valores[i]` devolvia `Unsupported("int")` — sem declaração real por
  trás do alias. Resolvido pedindo o tipo do argumento de template 0 do
  *próprio tipo do dono* (`vector<int>`, já disponível como `owner` em
  `lower_stdlib_method_call`) em vez do tipo da expressão de chamada —
  reaproveita exatamente a mesma resolução de tipo de elemento que
  `lower_type` já faz para um valor do tipo `vector<int>`.

- **`std::string::size()`/`std::vector::size()` devolvem `size_type`
  (`unsigned long` neste toolchain), não `int`.** `return mensagem.size();`
  (retorno `int`) e `i < valores.size()` (comparação com `int`) inserem uma
  conversão implícita C++ não coberta pelo único caso já tratado
  (`int`→`double`, do E04). Resolvido mapeando `CXType_ULong` para
  `Type::Int` em `lower_type` — mesma decisão que qualquer outra largura de
  inteiro já toma neste projeto (a divergência de largura fica para
  US-10/`equivalence.rs`, precedente do overflow de `int` do E01).

- **`clang_Cursor_Evaluate` devolve nulo para um cursor `StringLiteral`
  puro** — ao contrário de inteiro/`double`, que `evaluate_int_eval_result`/
  `evaluate_float_eval_result` já resolvem com a mesma API. Descoberto com
  `eprintln!` depois que `"Ola, " + nome` virava
  `_syntaxBridgeUnsupported(...) + nome` (a string literal falhando a
  virar `Expr::StringLiteral`, não o `operator+` — esse já discava
  certo). Corrigido tokenizando a extensão do próprio cursor
  (`clang_tokenize`) e lendo a soletração do primeiro token, em vez de
  `clang_Cursor_Evaluate` — só cobre os escapes que este corpus usa
  (`\\`, `\"`, `\n`, `\t`, `\r`), documentado como incompleto, não
  adivinhado como completo.

## Decisão de projeto tomada aqui

- **Todo parâmetro de biblioteca é `const T&`, nunca por valor.** Ver acima
  — decisão deliberada para não misturar duas armadilhas (encoding de
  string vs. cópia-por-valor) no mesmo degrau. `push_back`/mutação de
  `vector` também ficaram de fora por isso: o fixture só lê, nunca muta,
  então "`std::vector` copia, `List` não" nunca precisou de resposta aqui.
- **`operator+`/`operator==`/`operator!=` de `std::string` viram
  `Expr::Binary` direto, não uma chamada de método/função.** Dart já
  sobrecarrega os três com o mesmo significado em `String`, então não há
  ponte nenhuma a construir — só reconhecer que "operator+" resolvido para
  uma função de sistema (`clang_Location_isInSystemHeader`) com um operando
  `Type::Str` é, semanticamente, o mesmo `+` que `Expr::Binary` já sabe
  emitir.
- **`i++` do C++ virou `i = i + 1` no próprio fixture, não suporte novo no
  emissor.** O E02 já havia decidido isso (`examples/E02-controle-de-fluxo/`
  usa a mesma forma) — `CXUnaryOperator_PostInc`/`PreInc` continuam fora de
  escopo até algum degrau realmente forçar a decisão; usar `i++` aqui teria
  sido escopo emprestado do E05 para um problema que não é dele.
