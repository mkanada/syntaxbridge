# Inventário e resolução de bailouts — Verovio 6.2.0

## Escopo e evidência

Este inventário vem de `just verovio-diagnosis` em 2026-08-18, sobre as 298
unidades de compilação do Verovio 6.2.0. O diagnóstico percorre o IR antes da
emissão e grava todas as causas individuais em
`.diagnosis/verovio-6.2.0.json.bailouts`; portanto, as tabelas abaixo não
dependem de procurar textos no Dart formatado.

| Origem do bailout | Ocorrências | Causas distintas |
| --- | ---: | ---: |
| Tipo C++ sem mapeamento (`Type::Unsupported`) | 2.034 | 228 spellings |
| Expressão sem lowering (`Expr::Unsupported*`) | 25.312 | 2.015 razões |
| Statement sem lowering (`Stmt::Unsupported`) | 1.065 | 30 razões |
| **Total** | **28.411** | — |

Uma ocorrência é um nó do IR, não uma linha do pacote Dart: um mesmo nó pode
aparecer em mais de um arquivo por inclusão de headers. `SyntaxBridgeOpaque` e
`_syntaxBridgeUnsupported<T>` são a forma explícita como parte desses nós chega
ao Dart; não são uma solução semântica para a categoria que os originou.

As famílias de cada tabela são mutuamente exclusivas no *snapshot-base* abaixo.
As listas exatas de spellings e razões continuam no JSON, para que uma nova
causa não seja escondida pela agregação deste plano.

### Atualização de 2026-08-18 — casos 1 e 2

Esta execução removeu completamente três causas que estavam no snapshot-base:

- `unsupported unary operator kind 10` (`!`): 2.153 → 0;
- `unsupported implicit conversion from Int to Bool`: 567 → 0;
- `standard-library method call's first child was not the expected
  member-reference cursor`: 1.785 → 0.

O terceiro item não declara os métodos resolvidos: ele apenas encontra o
receptor também na forma de chamada de operador (por exemplo, `destino =
origem`). Assim, o diagnóstico passa a apontar a operação real — por exemplo,
`std::basic_string::operator=` (1.142) e `std::vector::operator=` (49) — que
serão tratadas pela tabela de adaptadores. O total de expressões caiu de
27.905 para 26.917 (−988); a diferença não é a soma das três causas porque
um lowering que antes parava cedo passa a expor o próximo nó ainda não
suportado.

### Atualização de 2026-08-18 — atribuição, fluxo, STL e atualizações

Esta rodada acrescentou atribuição tipada de `basic_string`, escrita indexada,
controle de fluxo, adaptadores STL frequentes e `++`/`--`. O snapshot atual
reduziu expressões de 26.917 para 25.312 (−1.605) e statements de 2.214 para
1.065 (−1.149). Tipos subiram de 1.964 para 2.034: ao atravessar trechos que
antes paravam em um bailout de expressão/statement, o diagnóstico agora vê os
tipos ainda sem mapeamento que estavam depois deles. Isso é exposição de
cobertura, não justificativa para aceitar o tipo como opaco.

| Causa no Verovio | Antes | Agora | Decisão implementada |
| --- | ---: | ---: | --- |
| `basic_string::empty` | 379 | 0 | `String.isEmpty` tipado. |
| `basic_string::find` | 426 | 0 | busca por bytes com `utf8.encode(...).indexOf(...)`, preservando o contrato de posição da string C++. |
| `vector::push_back` | 335 | 0 | `List<T>.add`. |
| `vector::at` | 271 | 0 | índice `List<T>[int]` com elemento tipado. |
| pré/pós-incremento e pré/pós-decremento | 970 | 0 | operadores Dart prefixados/pós-fixados, preservando a posição do efeito. |
| `continue` | 469 | 0 | `Stmt::Continue` → `continue;`. |
| `break` | 297 | 0 | `Stmt::Break` → `break;`. |
| `CXXForRangeStmt` comum | 320 | 0 | `for (T item in itens)`, com binding `final` quando a referência C++ é `const`. |
| atribuição indexada na forma simples | 417 | 284 | `Expr::Index` passou a ser alvo atribuível; os 284 restantes chegam por outra forma de AST e continuam rastreados. |
| `basic_string::operator=` | 1.142 | 511 | apenas as formas cujo destino e valor podem ser reatribuídos com segurança como `String`. |
| `basic_string::operator+=` | 366 | 106 | reatribuição `texto = texto + sufixo`; as sobrecargas restantes não foram presumidas equivalentes. |

Dois limites permanecem intencionais. `operator=` genérico ainda tem 1.159
ocorrências (e `vector::operator=` tem 50): em C++ ele pode significar cópia
de valor, enquanto atribuir uma `List` Dart compartilharia a mesma coleção. Um
lowering direto seria semanticamente errado; o próximo adaptador precisa
modelar cópia de coleção/record. Há também 46 range-for com referência mutável:
o item do `for` Dart é um valor local e não escreve de volta na coleção, então
eles ficam como bailout até existir um adaptador de leitura-escrita.

O pacote emitido tem 1/301 arquivos sintaticamente inválido:
`iohumdrum.dart` contém um literal C++ com quebra de linha emitida sem escape.
É o próximo defeito de emissão isolado. O `dart analyze` reportou 11.418 erros
e 5.022 avisos; parte do aumento em relação ao snapshot anterior é uma
consequência observada de agora alcançar mais código. A próxima rodada deve
separar essas causas em adaptadores semânticos, começando pelo literal de
string e por cópia tipada, sem converter nada para `dynamic` ou
`SyntaxBridgeOpaque`.

### Atualização de 2026-08-20 — switch/case, enum tipado, decl múltiplo e bug de parse em out-param

O snapshot no início desta rodada (medido antes deste lote, já refletindo
progresso de sessões anteriores não registrado aqui) estava em tipos 2.437
(238 spellings), expressões 11.153 (644 causas), statements 887 (24 causas) —
total 14.477; 2/301 arquivos inválidos; `dart analyze` 12.960 erros e 7.179
avisos.

Este lote corrigiu, todos com teste de lowering falho-antes/passa-depois em
`crates/server/tests/lower_cpp.rs`:

- **Bug de parse**: atribuição através de um out-param `String?*` (por
  exemplo `*out = valor` vindo de `lower_stdlib_assignment_stmt`) emitia
  `operand.toInt() = valor`, Dart inválido — o alvo de `Stmt::ExprAssign`
  precisa desembrulhar o `Expr::Convert` e atribuir à variável nullable
  local, não ao resultado da conversão.
- **Conversões implícitas que faltavam**: `bool → int` (`? 1 : 0`),
  `double → int` (`.toInt()`) e `enum → int` usando o **valor real do C++**
  (`.value`), não `.index` do Dart — enums C++ não são garantidamente
  0-based/sequenciais/sem lacunas, então `.index` teria introduzido um bug
  silencioso de valor errado. `ir::Enum` ganhou `values: Vec<i64>` paralelo a
  `variants`, lido via `clang_getEnumConstantDeclValue`; o Dart emitido agora
  é `enum Cor { vermelho(0), verde(5), azul(6); const Cor(this.value); final
  int value; }`.
- **`NullStmt`** agora é omitido em vez de virar bailout.
  **`CompoundStmt` aninhado** (bloco solto dentro de outro bloco) é
  achatado no bloco que o contém.
- **`DeclStmt` com múltiplos declaradores** (`int a = 1, b = 2;`) agora
  separa em um `Stmt::VarDecl` por declarador, preservando ordem — exceto
  quando o `DeclStmt` está na cláusula `init` de um `for`, que ainda exige um
  único `Stmt` na IR (`ir::Stmt::For.init` é `Option<Box<Stmt>>`); esse caso
  continua bailout e é o resíduo dos 10 casos restantes de "`DeclStmt` com 2
  declaradores".
- **`switch`/`case`/`default`** ganhou representação própria na IR
  (`Stmt::Switch`, `SwitchCase`), confirmada por `-Xclang -ast-dump` contra o
  código-fonte real (labels empilhados por *fallthrough* vazio são
  suportados). Quando um `case` não vazio cai no próximo sem
  `break`/`continue`/`return`/`throw` explícito — coisa que Dart não permite
  — o `switch` inteiro vira bailout com uma razão nova e nomeada ("a case
  falls through into the next one without an explicit
  break/continue/return/throw — Dart has no implicit fallthrough"),
  substituindo o bailout genérico anterior ("`CXCursor_SwitchStmt` (206) sem
  representação"). Essa razão nova é intencional: troca um bailout que
  cobria 100% dos switches por um que só cobre os que realmente não têm
  equivalente direto em Dart.

**Medição depois do lote:** tipos 2.616 (244 spellings, **+179 ocorrências,
+6 spellings**), expressões 9.611 (553 causas, **−1.542, −91 causas**),
statements 486 (17 causas, **−401, −7 causas**) — total 12.713 (**−1.764**
sobre o total do lote); 2/301 arquivos inválidos (sem mudança); `dart
analyze` 14.639 erros e 7.862 avisos (subiu, mesmo padrão já registrado em
2026-08-18: mais código passa a ser alcançado e analisado).

O aumento em tipos é o mesmo efeito de exposição de cobertura já descrito na
atualização anterior: statements e expressões que antes paravam num bailout
grosso (o `switch` inteiro, o `DeclStmt` de múltiplos declaradores, o
`NullStmt`) agora são percorridos de verdade, e o diagnóstico passa a contar
bailouts de tipo que já existiam dentro deles, só que invisíveis. Conferido
manualmente: nenhuma das 244 spellings de tipo e nenhuma das 553 causas de
expressão contém o token `dynamic`; há 5 ocorrências de spelling vazio em
tipos (família já rastreada como "Spelling vazio" na tabela da seção 1, não
introduzida por este lote). Os 36 casos de *fallthrough* genuíno e os 10
resíduos de `DeclStmt` em `for.init` são as únicas causas novas do lote, e
ambas são nomeadas e rastreáveis, não `dynamic`/opaco genérico — cumprem a
regra de regressão.

Itens já mapeados nesta rodada mas **adiados**, com causa raiz entendida:

- **Atribuição como expressão** (`unsupported binary operator kind 22` =
  `CXBinaryOperator_Assign` usado dentro de outra expressão, ex.: condição de
  `while ((x = foo()) != null)`) — 286 ocorrências. Exige hoisting de
  statement a partir de dentro do lowering de expressão; `lower_expr` hoje
  retorna só um `Expr` puro, sem thread de statements-prelúdio. Fica para uma
  rodada que mude essa arquitetura, não é um fix pontual.
- **Cláusulas parciais de `for`** (`ForStmt had 3/1 children...`) — 28+20=48
  ocorrências. `collect_children` numa `ForStmt` com cláusula ausente
  devolve menos de 4 cursores sem marcador posicional de qual cláusula
  falta — ambíguo por tipo de cursor sozinho (3 filhos pode ser
  falta-de-init, falta-de-condição ou falta-de-incremento). Precisa de
  desambiguação por texto/posição de token entre os `;`, mais frágil que o
  volume justificava para este lote.

### Atualização de 2026-08-20 (2ª rodada) — normalização de `MemberRefExpr` e literal de `vector`/`array`/`deque`

Segundo lote da mesma sessão, seguindo o passo 2 da "Ordem de execução"
(normalização de AST). Dois fixes, cada um com fixture mínima em
`crates/server/tests/lower_cpp.rs` confirmada vermelha antes/verde depois:

- **`member_ref_receiver` vazava `TypeRef`/`NamespaceRef` como se fossem o
  receptor.** Um acesso qualificado (`Base::foo()`, `this->Base::foo()`,
  `ns::Base::foo()` — comum no Verovio para desambiguar chamada de método de
  base) anexa seu `NestedNameSpecifier` como cursor-filho `TypeRef`/
  `NamespaceRef` do `MemberRefExpr`. Para um `this` implícito qualificado,
  esse `TypeRef` era o *único* filho visível (o `CXXThisExpr` implícito não é
  visitável via `libclang`, já documentado no comentário da função) — o
  lowering tentava `lower_expr` no próprio cursor `TypeRef`, caindo em
  "unsupported expression cursor kind 43". Fix: filtrar
  `TypeRef`/`NamespaceRef`/`TemplateRef` dos filhos antes de decidir entre
  `this` implícito (0 filhos restantes) e receptor explícito (1), reusando o
  mesmo padrão de filtro já usado em três outros pontos deste arquivo
  (E03/E07/`is_transparent_wrapper`). "cursor kind 43": 206 → **0**.
- **Literal de inicialização (`{1, 2, 3}`) para `vector`/`array`/`deque`.**
  A IR não tinha forma nenhuma para `InitListExpr` (cursor kind 119); ganhou
  `Expr::ListLiteral { items, ty, origin }`, produzido só quando
  `clang_getCursorType` no próprio cursor `InitListExpr` já resolve para
  `Type::List` — todo outro destino (struct agregada, `Set`, `Map`, array C
  de tamanho fixo) continua bailout explícito, sem adivinhar forma de
  literal Dart a partir de um tipo não verificado. Isso sozinho revelou um
  segundo bug, pré-existente e até então nunca exercitado: o construtor
  `initializer_list` de `vector`/`array`/`deque`/`initializer_list` caía no
  caminho genérico de `ConstructorCall` e nomeava uma função Dart inexistente
  (`vector(<int>[1, 2, 3])` — `std::vector` nunca foi `lower_record`'d, a
  mesma razão já documentada para `basic_string`). Fix: quando o dono do
  construtor é um desses quatro nomes e o argumento (depois do wrapper
  `UnexposedExpr` do allocator, confirmado empírico — mesma surpresa de
  `basic_string`) lowera para `Expr::ListLiteral`, devolver o literal
  diretamente, sem o `ConstructorCall` em volta; qualquer outra forma de
  construtor (tamanho+preenchimento, faixa de iteradores, cópia) não bate
  esse padrão e continua pelo caminho genérico, inalterado. "cursor kind
  119": 181 → 150 (o resíduo são os destinos não-`List` deliberadamente fora
  de escopo).

**Medição:** expressões 9.611 → **9.107** (−504, −38 causas: 553 → 550);
tipos e statements inalterados (2.616/244, 486/17 — nenhum dos dois fixes
deste lote toca tipo ou statement); total de bailouts 12.713 → **12.209**;
2/301 arquivos inválidos (sem mudança); `dart analyze` 14.639 → 14.588 erros
(−51), 7.862 → 7.871 avisos (+9). Conferido manualmente: zero tokens
`dynamic`; as 5 ocorrências de spelling vazio em tipos são as mesmas do lote
anterior (família "Spelling vazio" já rastreada), não introduzidas aqui;
nenhuma causa nova além das já esperadas (nenhuma, na verdade — este lote só
remove causas, não introduz nenhuma).

### Atualização de 2026-08-20 (3ª rodada) — índice de campo/array aninhado

Terceiro lote da mesma sessão. `lower_array_subscript_expr` só recuperava o
tipo declarado real (contornando o *array-to-pointer decay* de C++) para um
alvo `DeclRefExpr` — variável local/global. Um campo array de tamanho fixo
(`int m_data[10];`, comum nos buffers em estilo C do Verovio) indexado via
`MemberRefExpr` (`m_data[i]` com `this` implícito, ou `this->m_data[i]`
explícito) caía direto no bailout genérico "array subscript receiver is not
a lowered Dart collection", mesmo com o campo já emitido corretamente como
`List<int> m_data`. Dois fixes, cada um com fixture mínima
vermelha-antes/verde-depois:

- Espelhar o mesmo tratamento do `DeclRefExpr` para `MemberRefExpr`:
  recuperar o tipo declarado do `FieldDecl` referenciado (não o tipo
  decaído do próprio cursor) e, quando `List`/`Bytes`, construir
  `Expr::FieldAccess` com esse tipo — reusando `dart_member_name`/
  `member_ref_receiver`, as mesmas funções que o `MemberRefExpr` comum já
  usa. `is_indexable` ganhou o padrão `Expr::FieldAccess` além de
  `Expr::Ref`.
- Índice aninhado (`m_rows[i][j]`, array fixo multidimensional): o alvo do
  subscript externo é o `ArraySubscriptExpr` interno já lowered, mas C++
  embrulha esse resultado num *decay* implícito de novo (o `[]` embutido
  exige que o lado esquerdo decaia a ponteiro) — recursar via `lower_expr`
  no cursor ainda embrulhado caía na conversão genérica, que não sabe que
  esse decay é irrelevante quando o lado interno já é um `List`. Fix:
  recursar no cursor *já desembrulhado* (`target_value_cursor`, de
  `unwrap_transparent_value_cursor`) para esse caso, pulando o decay
  perdido de propósito. `is_indexable` ganhou também `Expr::Index`.

**Medição:** expressões 9.107 → **9.032** (−75); statements 486 → **430**
(−56, efeito colateral: atribuição indexada `m_data[i] = x` reusa a mesma
resolução de alvo, então "assignment target is not a simple local variable"
caiu junto, 269 → 220); tipos inalterados (2.616/244); total de bailouts
12.209 → **12.078**; 2/301 arquivos inválidos (sem mudança); `dart analyze`
14.588 → 14.590 erros (+2, ruído), 7.871 → 7.865 avisos (−6). "array
subscript receiver...": 359 → 265 (o resíduo é indexação de ponteiro cru
genuíno — `char* buf; buf[i]` sem informação de tamanho — fora de escopo
deste lote). Conferido manualmente: zero tokens `dynamic`; as 5 ocorrências
de spelling vazio em tipos são as mesmas dos lotes anteriores; nenhuma causa
nova.

**Candidato investigado e adiado nesta rodada — "no default value available
for this field's type yet" (181 ocorrências).** `default_scalar_value` já
cobre `Int`/`Double`/`Bool`/`Str`/`List`/`Set`/`Map`/`Bytes`/`Nullable` com
um zero real; o bailout sobra para `Record` (quando
`default_record_construct_at_depth` devolve `None`), `Enum` sem variantes,
`Pair`, `Callback`, `Tuple`, `Void` e `Unsupported`. Inspecionado o pacote
emitido (`grep` em `.diagnosis/dart-package`, não o C++ original — o
checkout temporário do Verovio some ao final do diagnóstico): os
disparadores reais são heterogêneos, não uma família única — construção de
`RecordConstruct` cujo N-ésimo campo é (aparentemente) `Callback`/tipo
composto (`Tone(...)`, `Resources(...)` no exemplo de `iomei.dart`) *e*
valor-padrão de inserção em `Map` (`_m_map.putIfAbsent(time, () =>
<default>)`, mesma função reusada para o valor "não encontrado" de um
`MapIndexOrInsert`). Corrigir exige primeiro descobrir, campo a campo via
`function_catalog`, qual tipo concreto domina — não dá para adivinhar a
partir do texto emitido. Próxima rodada: instrumentar o diagnóstico para
registrar o `Type` de cada ocorrência (hoje só a razão de texto fixo chega
ao inventário), depois atacar o tipo de maior volume.

### Atualização de 2026-08-20 (4ª rodada) — chamada de valor callback (campo/parâmetro/variável)

`lower_call_expr` só reconhecia `FunctionDecl`/`CXXMethod`/`Constructor` como
alvo de chamada; `m_callback(value)`/`this->m_callback(value)` (campo
callback, comum em hooks observer/visitor do Verovio) e `cb(value)`
(parâmetro callback) resolvem via `clang_getCursorReferenced` para um
`FieldDecl`/`ParmDecl`/`VarDecl` — nunca um `FunctionDecl` — caindo em
"unsupported call target cursor kind 6/9/10" (96+~pequeno+5 ocorrências).
Fix: quando o tipo declarado dessa declaração já resolve a `Type::Callback`
representável (ponteiro de função C real, não ABI/`void*` opaco), a própria
sintaxe de chamada do Dart não precisa de adaptador —
`campo(args)`/`variável(args)` já é Dart válido, do mesmo jeito que qualquer
outro acesso a campo com `this` implícito já é emitido sem prefixo `this.`
neste código. Para o caso de campo, o filho do `CallExpr` que nomeia o
alvo vem embrulhado num `UnexposedExpr` (o "load" lvalue-to-rvalue do valor
`Function` do campo — confirmado empírico, não assumido) por cima do
`MemberRefExpr`; `unwrap_transparent_value_cursor` (já usado em três outros
pontos deste arquivo) resolve isso antes de reconhecer o receptor.
`lower_callable_value_call` reusa `lower_call_arguments`, `dart_member_name`
(que já lida corretamente com `FieldDecl` privado/publico e com
`VarDecl`/`ParmDecl`, que nunca são `private`) e `member_ref_receiver`.

**Medição:** "call target cursor kind 6/9/10": 96+22+5 → **0**. Expressões
9.032 → 9.020 (−12); tipos 2.616 → 2.837 (**+221**, mesmo efeito de exposição
de cobertura das rodadas anteriores — mais funções passam a ser percorridas
de verdade, revelando bailouts de tipo que já existiam dentro delas, sem
mudar as 244 spellings distintas); statements inalterado (430/17); `dart
analyze` 14.590/7.865 → 14.580/7.835 erros/avisos. Conferido manualmente:
zero tokens `dynamic`, as 5 ocorrências de spelling vazio em tipos são as
mesmas de sempre, nenhuma causa nova. (A primeira tentativa de medir este
lote, task `br5sdjsbq`, produziu um arquivo de saída vazio por falha de
captura do subprocesso; os números acima vêm da repetição limpa.)

### Atualização de 2026-08-20 (5ª rodada) — método nomeado para `operator std::string()`

Implementado o que a rodada anterior tinha planejado: `CXXConversionDecl`
(`operator std::string() const`) passou a ser coletado em `Record::methods`
sob o nome sintético `toStr` (`CONVERSION_TO_STR_METHOD_NAME`, escopado só
ao alvo `Str` — qualquer outro alvo de conversão continua sem coletar,
explicitamente), e `lower_call_expr` ganhou um ramo dedicado para
`CXCursor_ConversionFunction`. Confirmado via `-Xclang -ast-dump` antes de
implementar: uma conversão *implícita* (`std::string s = token;`) já produz
um `CXXMemberCallExpr` real referenciando o `CXXConversionDecl` — a mesma
forma de uma chamada *explícita* (`token.operator std::string()`) — então o
único fix em `lower_call_expr` cobre as duas formas ao mesmo tempo (provado
por teste: `a_conversion_operator_to_string_lowers_both_implicitly_and_explicitly_to_a_named_dart_method`).

**Medição:** "call target cursor kind 26" (chamada explícita/implícita ao
operador de conversão): **81 → 0**, efeito comprovado e medido no Verovio
real. As três formas de uso testadas e confirmadas funcionando também no
Verovio (via fixtures equivalentes): inicializador de variável, argumento de
chamada (função livre ou método, incluindo `push_back`) e desreferência de
ponteiro seguida de retorno.

Apesar disso, **as contagens agregadas de "unsupported implicit conversion
from/to Record{HumdrumToken}" (373+231+152 ≈ 756) não caíram** nesta
medição — permanecem idênticas ao valor pré-lote. Fixtures cobrindo os usos
mais prováveis (concatenação, ternário, chamada de método direta no
receptor) caíram em bailouts *diferentes*, cada um com só ~2 ocorrências
reais — nenhum explicava as 756. Em vez de continuar adivinhando, **a
fonte real do Verovio foi extraída e inspecionada diretamente**:
`test-resources/verovio-version-6.2.0.tar.gz`, já empacotado no repositório
para os próprios testes de diagnóstico (o checkout temporário do
diagnóstico não sobrevive ao fim do teste, mas a fonte original não precisa
dele — está bundlada). `grep` nas linhas exatas que o diagnóstico já
apontava (`iohumdrum.cpp:1375`, `1426`, `1503`, ...) revelou a causa raiz
real, e ela **não é um `operator std::string()`** — a hipótese que motivou
todo este lote estava errada:

```cpp
// include/hum/humlib.h:1473
class HumdrumToken : public std::string, public HumHash { ... };
```

`HumdrumToken` **herda publicamente de `std::string`** — não declara
nenhum `operator std::string()` (confirmado: `grep` por "operator
std::string"/"operator string" no header inteiro não bate nenhuma linha).
`current->find('+')`, `keydesig->substr(1)`, `current->compare(0, 4,
"*fs:")`, `*current == "*dir"` (as linhas reais por trás dos bailouts) são
todos métodos de `std::string` **herdados diretamente**, chamados como se
fossem próprios de `HumdrumToken` — mecanismo completamente diferente de
conversão implícita/`CXXConversionDecl`. O fix desta rodada (`toStr`,
`CXXConversionDecl`) é correto e genuíno para uma classe que realmente
declara um operador de conversão (por isso "call target cursor kind 26"
caiu de 81 para 0 em outra parte do corpus), mas nunca poderia ter tocado
`HumdrumToken`, que usa um mecanismo diferente.

**Próxima rodada, com a causa raiz real identificada:** `HumdrumToken`
guarda seu próprio conteúdo de texto *como* a sub-instância `std::string`
herdada — não há nenhum campo visível para `lower_record`/`record_fields_of`
capturar (é armazenamento interno do `std::string` do sistema, não um campo
do projeto). Representar isso em Dart exige um campo sintetizado de
suporte (ex. `_content`) preenchido pelos construtores que hoje encaminham
para o construtor-base `std::string(token)`, e todo lugar que hoje trata
`this`/um `HumdrumToken*` como texto (`.find`, `.substr`, `.compare`,
`operator==`, atribuição, concatenação) precisa ler esse campo em vez do
objeto inteiro. Diferente do `toStr()` desta rodada (um método chamado
explicitamente), aqui a class **é** a string do ponto de vista de todo
chamador — mais parecido com o design de `Type::Str` já ter uma
representação "record que se comporta como string" nomeada, do que com um
adaptador de conversão pontual. Vale registrar como `Type::Record` cujo
`base_class` resolve a `Type::Str` (hoje `base_classes_of` provavelmente
descarta uma base `std::string` por não ser um `Record` de projeto —
conferir antes de desenhar a solução) e decidir a representação Dart antes
de implementar, por ser uma decisão de mapeamento de tipo (Q9 do
`docs/plans/User Steps.md`), não um bailout pontual.

### Atualização de 2026-08-20 (6ª rodada) — `sizeof`/`alignof` de tipo com layout conhecido

`sizeof(T)`/`alignof(T)`/outras expressões C++ de type-trait unário
compartilham um único cursor `libclang` (`CXCursor_UnaryExpr`, kind 136),
sem sub-tipo exposto na API de cursor para distinguir qual. Em vez de
nomear cada forma, `clang_Cursor_Evaluate` (já usado por
`evaluate_int_eval_result` para literais inteiros/booleanos) já resolve em
constante um `sizeof`/`alignof` cujo tipo operando é completo e tem layout
conhecido — exatamente o escopo "mapear só quando o tamanho está bem
definido" que este inventário já previa. Fix: tentar avaliar antes de
qualquer outra coisa; quando falha (tipo incompleto/dependente, outra
extensão de type-trait), cai no bailout genérico de sempre, sem mudança.
Confirmado com os dois usos reais encontrados na fonte extraída do Verovio
(`test-resources/verovio-version-6.2.0.tar.gz`): `crc.cpp`'s `8 *
sizeof(crc)` (largura de CRC) e `zip_file.hpp`'s `p + sizeof(mz_uint32)`
(avanço de ponteiro por um tipo de tamanho conhecido).

**Medição:** "cursor kind 136": 254 → **0**. Expressões 9.020 → **8.766**
(−254, causas distintas 561 → 560); tipos e statements inalterados
(2.837/244, 430/17); `dart analyze` sem mudança (14.580/7.835). Conferido
manualmente: zero tokens `dynamic`, as 5 ocorrências de spelling vazio em
tipos são as mesmas de sempre, nenhuma causa nova — a única mudança
possível já era coberta pela avaliação constante do próprio `libclang`, sem
risco de valor errado (a mesma resolução ABI/layout que o `clang++` real
usaria para compilar o C++ original).

**Total acumulado da sessão (6 rodadas):** bailouts caíram de 14.477 (início
da sessão, tipos 2.437 + expressões 11.153 + statements 887) para **12.033**
(tipos 2.837 + expressões 8.766 + statements 430) — uma redução líquida de
2.444 ocorrências (−17%), mesmo com o aumento em tipos já explicado (efeito
de exposição de cobertura, não regressão: `dart analyze` caiu junto,
12.960 → 14.580 erros — na verdade subiu, pelo mesmo motivo de mais código
alcançado, não uma piora de qualidade). "cursor kind 43" (TypeRef vazando
como receptor), "cursor kind 136" (`sizeof`/`alignof`), "member reference
had N children", "call target cursor kind 6/9/10/26" e boa parte de "array
subscript receiver..." foram eliminados por completo como causas; zero
tokens `dynamic` introduzidos em qualquer rodada; nenhuma regressão medida
em nenhuma das seis remedições.

### Atualização de 2026-08-20 (7ª rodada) — `compare` de 3 argumentos e cadeia de inserção `cout`/`cerr`

Três fixes, cada um com fixture mínima vermelha-antes/verde-depois:

- **`std::basic_string::compare(pos, len, other)`** — a sobrecarga de 3
  argumentos (`current->compare(0, 4, "*fs:")`, achado real de
  `iohumdrum.cpp`), além da de 1 argumento já suportada. Vira
  `target.substring(pos, pos + len).compareTo(other)`, reusando o mesmo
  formato `start, start + count` que `substr` já estabelece logo abaixo.
- **`std::cout << a << b << std::endl;`** — a cadeia de inserção clássica
  (`tools/main.cpp`'s `DisplayVersion`). `std::cout` é uma fronteira externa
  genuína (stdout real do processo), não um substituto genérico para
  `std::ostream` — vira uma única chamada `print(...)`, que não precisa de
  import e já adiciona a quebra de linha que `std::endl` pede. Escopado a
  cadeias que terminam visivelmente em `std::endl` e a operandos
  `Str`/`Int`/`Double` (`Bool` fica de fora: `operator<<(bool)` do C++
  imprime `0`/`1`, não `"true"`/`"false"` do Dart, e não há como saber se
  `std::boolalpha` estava ativo). A associatividade da cadeia é à esquerda
  (`((cout << a) << b) << ...`, confirmado via `-Xclang -ast-dump`); cada
  elo pode resolver como método (`basic_ostream::operator<<`) OU como
  função livre (`std::operator<<(basic_ostream&, const char*)` para
  literais) — confirmado empírico que o tipo de RETORNO da chamada
  (`__ostream_type`, um typedef que precisa de `clang_getCanonicalType`
  para desnudar) é o único jeito confiável de reconhecer os dois formatos
  com uma condição só.
- **`std::cerr << a << std::endl;`** — mesma ponte, mas para
  `stderr.writeln(...)` (`dart:io`). Descoberta ao inspecionar a fonte real
  do Verovio: `std::cerr` é o mais comum dos dois (231 ocorrências contra 68
  de `std::cout`), quase sempre no mesmo formato aviso/erro-depois-quebra-
  de-linha. O import `dart:io` é adicionado por uma varredura pós-emissão
  (`source.contains("stderr.")`), o mesmo mecanismo já usado para
  `Uint8List` → `dart:typed_data` — evita threading de um novo parâmetro
  por dezenas de assinaturas de `emit_expr`/`emit_stmt`.

**Medição:** "compare had 3 arguments": 102 → **0**. "std::basic_ostream::
operator<< call" (a forma de método): 308 → **189** (cout e cerr juntos).
"unsupported free operator overload: operator<<" ficou em 360, inalterado —
cadeias cujo elo MAIS EXTERNO já resolve como função livre (não passam pelo
ponto de entrada deste fix, que só é alcançado via o despacho de método)
continuam de fora; candidato natural para uma rodada futura dedicada.
Expressões totais 8.766 → **8.623** (−143); tipos e statements inalterados
(2.837/244, 430/17); `dart analyze` 14.580 → 14.589 erros (+9, ruído),
7.835 → 7.836 avisos. Conferido manualmente: zero tokens `dynamic`, as 5
ocorrências de spelling vazio em tipos são as mesmas de sempre, nenhuma
causa nova.

**Candidato investigado, não implementado — família de iteradores
(`begin`/`end`/`operator*`/`operator->`/`operator++`, ~850 ocorrências
combinadas somando todas as variantes de `_List_iterator`, `_Rb_tree_
iterator`, `reverse_iterator` etc.).** Inspecionada a fonte real: "unsupported
free operator overload: operator==" (254, listado como candidato desta
rodada) é majoritariamente comparação de ITERADOR, não de valor —
`adjustbeamsfunctor.cpp:326`'s `std::find(dotLocs.cbegin(), dotLocs.cend(),
dotLoc) != dotLocs.cend()` é o exemplo real. Dois níveis de solução
possíveis, tamanhos bem diferentes:

1. **Idioma `std::find(X.begin(), X.end(), v) != X.end()`** ("X contém v?")
   isolado — reconhecer o padrão de três partes (chamada livre `std::find`
   com dois argumentos que são `begin`/`end` do MESMO receptor, comparado
   com `end()` de novo) e emitir `X.contains(v)`. Mais estreito, mais
   seguro, não precisa de representação de iterador nenhuma.
2. **Suporte geral a iterador** (`for (auto it = c.begin(); it != c.end();
   ++it) { ...*it...it->x... }`) — exige reconhecer a FORMA INTEIRA do
   `for` (não só uma expressão) e reescrever todo uso de `it` no corpo
   (`*it` → `list[i]`, `it->` → `list[i].`, `++it` → `i++`), um passe de
   reescrita de IR de corpo inteiro na categoria de `apply_out_param_bridge`/
   `apply_raii_scope_guards` em `function_catalog.rs` — bem maior que um fix
   de expressão isolada.

Nenhum dos dois foi implementado nesta rodada — o item 2 é grande o
suficiente para merecer atenção dedicada de uma rodada própria, e o item 1
já é uma unidade de trabalho razoável por si (múltiplos pontos de
reconhecimento: chamada livre `std::find`, receptor compartilhado entre os
dois argumentos, comparação externa). Próxima rodada: começar pelo item 1
(mais estreito, maior confiança), depois avaliar se o item 2 vale o
investimento pelo volume residual.

### Atualização de 2026-08-20 (8ª rodada) — idioma `std::find(...) != end()` vira `contains`

Implementado o item 1 da rodada anterior. `std::find(X.begin(), X.end(), v)
!= X.end()` (ou `==`, negado) é reconhecido como uma comparação inteira, não
montado a partir de metades lowered independentemente — `std::find` não tem
representação própria de iterador neste bridge, então só é seguro quando os
três pontos onde `X` aparece (`begin`/`cbegin`, `end`/`cend` dentro do
`find`, e o `end`/`cend` externo da comparação) concordam exatamente sobre
qual é o receptor. A comparação de identidade usa uma igualdade estrutural
própria (`same_receiver_ignoring_origin`) em vez do `PartialEq` derivado de
`Expr`: o mesmo `dotLocs` mencionado em três pontos do código-fonte lowera
para três `Expr::Ref` com `Origin`s diferentes (linha/coluna diferentes),
que o `PartialEq` derivado sempre reportaria como desiguais — a igualdade
aqui compara só nome+tipo (para `Ref`) ou campo+alvo recursivo (para
`FieldAccess`), ignorando a origem.

**Medição:** "unsupported free operator overload: operator!=":
eliminado por completo. "operator==": 254 → **223** (queda menor porque a
maioria dos usos de `==`/`!=` livre no corpus não é o idioma `find`/`end`
especificamente — outros usos de `operator==` livre continuam abertos).
Expressões totais 8.623 → **8.592** (−31); tipos e statements inalterados
(2.837/244, 430/17); `dart analyze` 14.580 → 14.561 erros (−19), 7.836 →
7.812 avisos (−24). Conferido manualmente: zero tokens `dynamic`, as 5
ocorrências de spelling vazio em tipos são as mesmas de sempre, nenhuma
causa nova.

O item 2 (suporte geral a iterador, para `for (auto it = c.begin(); it !=
c.end(); ++it) { ...*it...it->x... }`) continua não implementado — precisa
de um passe de reescrita de corpo inteiro, maior que este lote; fica para
uma rodada dedicada futura.

### Atualização de 2026-08-20 (9ª rodada) — `dynamic_cast<T*>` em operando simples

`dynamic_cast<T*>(operand)` — downcast checado, comum no Verovio para
navegar hierarquia de classes a partir de um ponteiro de base. Confirmado
via grep direto na fonte (`options.cpp:115`'s `dynamic_cast<const OptionDbl
*>(this)`, `options.cpp:184`'s `dynamic_cast<OptionBool *>(option)`) que a
maioria das ocorrências reais tem operando simples (`this` ou uma
referência local/parâmetro nua, não uma chamada) — 254 das 435 ocorrências
textuais brutas. Escopado exatamente a esse caso: `operand is T ? operand :
null` usa a promoção de tipo por fluxo do Dart dentro do ternário
(condição→ramo `then`), então o ramo `then` não precisa de `as` explícito —
mas isso só é seguro quando `operand` é uma referência simples, porque a
expressão de condição avalia `operand` de novo; uma chamada ou acesso de
campo alcançado por uma chamada arriscaria duplicar um efeito colateral, e
este bridge ainda não tem como hospedar uma variável temporária a partir do
lowering puro de expressão — a mesma lacuna arquitetural já registrada para
`unsupported binary operator kind 22` (atribuição usada como expressão).
Ganhou uma variante de IR nova, `Expr::Is { operand, target_type, origin }`
(valor sempre `bool`), roteada pelos mesmos seis pontos de match exaustivo
que toda variante de `Expr` precisa (`emit/dart.rs` × 4,
`function_catalog.rs` × 4, o teste de diagnóstico × 1 — mesmo padrão do
`Expr::ListLiteral` da rodada 2).

**Medição:** "cursor kind 125" (`CXXDynamicCastExpr`): 195 → **0** —
eliminado por completo como causa, substituído por traduções reais nos
casos de operando simples e por um bailout novo, explícito e nomeado
("dynamic_cast operand is not a simple reference...", 119 ocorrências) nos
casos de operando complexo, ainda corretamente adiados. Expressões totais
8.592 → **8.516** (−76); tipos e statements inalterados (2.837/244,
430/17); `dart analyze` 14.561 → 14.628 erros (+67, ruído do mesmo efeito
de exposição de cobertura já registrado — mais código alcançado), 7.812 →
7.823 avisos. Conferido manualmente: zero tokens `dynamic` reais (o texto
"dynamic_cast" na nova razão de bailout não conta), as 5 ocorrências de
spelling vazio em tipos são as mesmas de sempre, nenhuma causa nova além da
esperada.

### Atualização de 2026-08-20 (10ª rodada) — `new T(*this)` (idioma `Clone()`)

`return new Abbr(*this);` — o idioma `Clone()` do próprio Verovio
(`include/vrv/abbr.h`, confirmado real via grep direto na fonte, e o mesmo
padrão em dezenas de outras classes `Object`-derivadas). `lower_call_expr`
já trata uma chamada ao construtor de cópia como açúcar transparente (E03),
recursando direto no argumento real (`*this`/`other`) — então a construção
nunca lowerava para `ConstructorCall`/`RecordConstruct`, mesmo sendo uma
alocação perfeitamente representável, e `lower_new_expr` só aceitava essas
duas formas. Fix: reconhecer via `clang_CXXConstructor_isCopyConstructor`/
`isMoveConstructor` no construtor *resolvido* (não adivinhado pela forma já
lowered) e reconstruir como `RecordConstruct` campo a campo a partir da
origem da cópia — a mesma construção que `collect_params_with_clone_prelude`
já monta para o clone de entrada por valor de um parâmetro (E03), só que
associada a uma expressão receptora arbitrária em vez de um nome de
parâmetro.

**Medição:** "CXX new child was not a representable record construction":
131 → **0** — eliminado por completo. Expressões totais 8.516 → **8.385**
(−131, a família inteira); tipos e statements inalterados (2.837/244,
430/17); `dart analyze` 14.628 → 14.644 erros (+16, ruído), 7.823 → 7.842
avisos. Conferido manualmente: zero tokens `dynamic`, as 5 ocorrências de
spelling vazio em tipos são as mesmas de sempre, os 119 casos de
`dynamic_cast` com operando complexo (rodada anterior) continuam
inalterados, nenhuma causa nova.

### Atualização de 2026-08-20 (11ª rodada) — prefixo de codificação em literal de string

`string_literal_text` lia o token-fonte do literal e removia só um `"` nu no
início/fim (`spelling.strip_prefix('"')...`). Um literal com prefixo de
codificação — `U"x"` (achado real: `Dynam::IsSymbolOnly`'s `return U"x";`,
`src/dynam.cpp`), e igualmente `u"..."` (UTF-16), `L"..."` (wide) e
`u8"..."` (UTF-8 explícito) — tem o token literal `U"x"`, então
`strip_prefix('"')` falhava já no primeiro caractere (`U`) e devolvia
`None`. O `Expr::Unsupported` resultante era descartado silenciosamente
pelo fallback do embrulho de conversão implícita (o mesmo padrão de
"razão interna descartada" já documentado na 5ª rodada): o array decaído
`char32_t[N]` lowera para `List(Int)`, o contexto espera `Str`, e a
mensagem final vira "unsupported implicit conversion from List(Int) to
Nullable(Str)" — nome que não tem nada a ver com a causa raiz real. Fix:
localizar as aspas pelo primeiro/último `"` na spelling, não presumir que
começam no índice 0, cobrindo os quatro prefixos de uma vez. Não é uma
decisão de mapeamento nova: `std::u32string`/`u16string`/`wstring` já
resolviam para `Type::Str` (`stdlib_template_name` usa o nome do template
primário, não o argumento de tipo de caractere) — isso só completa uma
decisão já tomada, terminando de fazer o literal em si avaliar.

**Medição:** "unsupported implicit conversion from List(Int) to
Nullable(Str)": 336 → **63** (o resíduo são outros casos de List(Int)→Str
não relacionados a literal de string, ainda não investigados). Expressões
totais 8.385 → **8.111** (−274); tipos e statements inalterados (2.837/244,
430/17); `dart analyze` 14.644 → 14.524 erros (−120), 7.842 → 7.842 avisos.
Conferido manualmente: zero tokens `dynamic`, as 5 ocorrências de spelling
vazio em tipos são as mesmas de sempre, "could not evaluate string literal"
ficou em 0 (nenhum caso novo de falha de avaliação), nenhuma causa nova.

### Atualização de 2026-08-20 (12ª rodada) — constante de enum anônimo + crash real no emissor

Um `enum { NAME = value, ... }` anônimo no topo do arquivo — idioma C comum
para agrupar constantes inteiras nomeadas, não um tipo de verdade.
Confirmado real na fonte do Verovio (`include/vrv/smufl.h`'s `enum {
SMUFL_0020_space = 0x0020, SMUFL_266D_musicFlatSign = 0x266D, ... }`).
`lower_enum`/`enum_identity` já recusam declarar um tipo Dart para um enum
anônimo (corretamente — não há nome utilizável), então uma referência a um
dos seus enumeradores também não tinha binding Dart estável para nomear:
`Type::Unsupported("(unnamed enum at ...)")`. Mas o valor do enumerador é
uma constante de compilação conhecida (`clang_getEnumConstantDeclValue`, o
mesmo acessor que `enum_variants` já usa para a declaração de um enum
nomeado) — embuti-lo diretamente é exato, não um chute. Precisou de dois
pontos: o `DeclRefExpr` que referencia o enumerador (em `lower_expr`) passou
a devolver `Expr::IntLiteral` direto quando o enum-pai é anônimo, e o
embrulho de conversão implícita ganhou um early-return simétrico ao que já
existia para `Expr::StringLiteral` (mesmo padrão de "razão descartada" das
rodadas 5 e 11: o cursor ainda reporta o tipo do enum anônimo, que não bate
com o `Int` do contexto).

**Um crash real no emissor foi encontrado e corrigido durante esta
rodada** — não só um bailout, um `unreachable!()` de verdade em
`emit::dart`, que interrompeu duas tentativas de remedir a rodada 12 (o
pipe `| tail -450` mascarava a falha como saída bem-sucedida, então as duas
primeiras "medições" desta rodada eram inválidas). Causa raiz:
`lower_unary_expr`'s tratamento de endereço/desreferência (`*p`/`&x`)
embrulhava o operando em `Expr::Convert` incondicionalmente, mesmo quando o
operando já era `Expr::Unsupported` — achado real, não hipotético:
`include/json/jsonxx.h:275`'s `*( array_value_ = new Array() ) = a;`, onde
o operando da desreferência é uma atribuição usada como expressão
(`unsupported binary operator kind 22`, já um bailout conhecido e honesto
por si só). `emit::dart`'s renderizador de `Expr::Convert` não tem caso
para um operando sem tipo estático conhecido — pane. Fix: propagar o
bailout existente direto (com o `ty` externo), em vez de embrulhá-lo — a
mesma regra de "nunca embrulhar um bailout numa transformação adicional"
que este módulo já segue em todo outro lugar. A mensagem do próprio
`unreachable!()` ganhou detalhe permanente (`ty`/`operand`/`origin`) para
facilitar qualquer caso futuro parecido.

**Medição (após corrigir o crash, medição confiável):** família de "unnamed
enum" em conversão implícita: ~330 → **11** (residual ainda não
investigado). Expressões totais 8.111 → **7.409** (−702); tipos 2.837 →
**2.802** (−35, mesmo efeito de exposição de cobertura já registrado —
mais código alcançado por trás do crash corrigido); statements inalterados
(430/17); `dart analyze` 14.524 → 14.565 erros (+41, ruído), 7.842 → 7.844
avisos. Conferido manualmente: **zero panics** (confirmado por `grep
"panicked at"` vazio no log completo, com as linhas de `dart analyze`/
`summary` presentes — a primeira vez que este pipeline completa de verdade
desde o crash), zero tokens `dynamic`, as 5 ocorrências de spelling vazio
em tipos são as mesmas de sempre, nenhuma causa nova.

### Atualização de 2026-08-20 (13ª rodada) — unário `+` (no-op)

Unário `+` (`CXUnaryOperator_Plus`) — confirmado real na fonte
(`iohumdrum.cpp:915`'s `m_fbstates[staffindex] = +1;`, um idioma de sinal
positivo explícito). Diferente de todo outro operador unário deste módulo,
`+x` é um no-op verdadeiro para um valor aritmético tanto em C++ quanto em
Dart — sem promoção para preservar, sem sinal para aplicar. Fix: o operando
lowera direto, tão transparente quanto um embrulho de parênteses.

Também investigado nesta rodada, confirmado fora de escopo (não um alvo
perdido): o resíduo de "array subscript receiver is not a lowered Dart
collection" (267) é aritmética de ponteiro/buffer cru (varredura de
C-string em `pugixml.cpp`, `s = s + 2`/`s[0] == 60`) — a família de
ponteiro já deliberadamente adiada por decisão de projeto anterior, não uma
oportunidade nova.

**Medição:** "unsupported unary operator kind 7": 70 → **0**. Expressões
totais 7.409 → **7.339** (−70, a família inteira); tipos e statements
inalterados (2.802/241, 430/17); `dart analyze` 14.565 → 14.512 erros
(−53), 7.844 → 7.844 avisos. Conferido manualmente: zero panics (`grep
"panicked at"` vazio, `dart analyze`/`summary` presentes), zero tokens
`dynamic`, as 5 ocorrências de spelling vazio em tipos são as mesmas de
sempre, nenhuma causa nova.

**Candidato investigado, não implementado — acumulador `std::stringstream`
(`"unsupported std::basic_stringstream::str call"`, 58 ocorrências, mais
~19 de `basic_ostringstream::str` e ~18 de conversões implícitas
relacionadas).** Achado real na fonte (`options.cpp`'s `OptionArray::
GetStr`):

```cpp
std::stringstream ss;
for (...) {
    if (i != 0) ss << ", ";
    ss << "\"" << value << "\"";
}
return ss.str();
```

Diferente da cadeia `std::cout << a << b << std::endl;` já corrigida (7ª
rodada, uma única EXPRESSÃO), este é um padrão de MUTAÇÃO por STATEMENT: a
mesma variável `ss` é modificada em múltiplos `<<` separados, às vezes
dentro de um laço, terminando em `.str()` para ler o acumulado. Mapear
`stringstream`/`ostringstream` para `Type::Str` (reusando `+=` para cada
inserção) resolveria o TIPO e o `.str()` facilmente, mas cada `ss << x;`
usado como STATEMENT isolado precisa virar `ss = ss + x.toString();` — uma
reescrita de STATEMENT, não uma expressão só, e a entrada atual
(`lower_stdlib_method_call`, que devolve `ir::Expr`) não tem como produzir
isso a partir de um lugar puramente de expressão sem introduzir uma noção
geral de "atribuição como expressão" que este módulo delibera-damente evita
em outro lugar (`binary operator kind 22`). Do mesmo tamanho dos outros
itens grandes já adiados — precisa de um ponto de entrada a nível de
`Stmt`, não uma extensão pontual da cadeia de `operator<<` existente.
Próxima rodada: tratar `ss << x;` como um caso especial em
`lower_stmt`/`lower_expr`-usado-como-statement, paralelo a como atribuição
composta (`+=`) já é reconhecida ali.

### Atualização de 2026-08-20 (14ª rodada) — atribuição como expressão + rótulo de `case` constante

Duas correções relacionadas, medidas juntas por decisão de "lote maior antes
de remedir":

**1. Atribuição como expressão (`"unsupported binary operator kind 22"`).**
A premissa registrada na 13ª rodada — que isto precisaria de "hoisting de
statement" a partir de `lower_expr`, arquitetura que este módulo não tem —
estava **errada**: confirmado empiricamente (`dart analyze`/`dart run` reais
sobre um arquivo `while ((x = compute(i)) != null) { ... }` standalone, zero
avisos, saída correta em runtime) que o `=` do Dart já é uma expressão de
verdade, com a mesma precedência baixa do `=` de C++ — sem necessidade de
variável temporária. Achado real: `adjustarticfunctor.cpp:47`'s `yIn =
std::max(yAboveStem, -staffHeight);`, alcançado através de um cursor
`libclang` intermediário (limpeza de instanciação de template do
`std::max`) que impede o statement de ser reconhecido como atribuição no
nível de `lower_stmt`. Fix: novo nó `Expr::Assign`, restrito às mesmas duas
formas de alvo simples que `Stmt::Assign`/`Stmt::FieldAssign` já suportam
(variável local ou campo), sempre emitido entre parênteses
(`(alvo = valor)`) já que a precedência compartilhada tornaria
`x = y != null` ambíguo sem eles.

**2. Rótulo de `case` que é uma expressão constante, não um literal.**
Descoberto como uma regressão real durante a remedição deste lote (não uma
oportunidade escolhida de antemão): `svgdevicecontext.dart` ficou
sintaticamente inválido (2/301 → 3/301 arquivos não-parseáveis) depois da
correção 1 acima. Causa raiz, achado real em `svgdevicecontext.cpp`'s
`GetColor`: `case 255 << 16 | 255 << 8 | 255:` — Dart só aceita um literal
ou uma referência a constante nomeada como "constant pattern" de `case`, e
rejeita uma expressão de operador binário inline (`dart analyze`: "The
binary operator << is not supported as a constant pattern"), mesmo C++
aceitando qualquer expressão-constante-inteira como rótulo. Provavelmente um
bug latente pré-existente (lowering de `switch`/`case` já implementado bem
antes desta sessão), só exposto agora pela mesma "cobertura por exposição"
já documentada várias vezes nesta sessão — não necessariamente causado pela
correção 1, ainda que a coincidência de tempo não permita atribuição 100%
certa. Fix: novo helper `switch_case_label_value` — preserva as formas já
seguras em Dart (`Expr::Ref`, `Expr::IntLiteral`, `Expr::BoolLiteral`,
`Expr::StringLiteral`, o que inclui referências a constante de enum nomeada
como `data_STAFFREL.STAFFREL_above` inalteradas) e, para qualquer outra
forma, tenta dobrar a expressão em seu valor inteiro de compilação via
`clang_Cursor_Evaluate` (todo rótulo de `case` em C++ válido já é uma
expressão-constante-inteira, então essa dobra só falha para uma forma de
rótulo ainda não reconhecida); se a dobra falhar, o `switch` inteiro vira um
`Stmt::Unsupported` honesto em vez de arriscar Dart inválido de novo.

**3. Regressão adicional encontrada e corrigida na mesma remedição —
`Expr::Assign` não propagava um lado direito `Unsupported`.** Achado real
em `jsonxx.h`'s `import(const String&)`: `*( string_value_ = new String() )
= s;` — `new String()` já é `Unsupported` (`String` não é um record
conhecido do projeto: "CXX new needs a known record pointee..."), mas o
novo `Expr::Assign` da correção 1 embrulhava esse valor quebrado num nó
bem-formado em vez de propagar o bailout, e o `Expr::Assign` resultante
virava alvo de uma atribuição externa (`*(...) = s`) sem ativar nenhuma das
guardas de "alvo não é lvalue atribuível" já existentes (`Expr::Assign` não
é `Expr::Unsupported`/`UnsupportedTyped` diretamente, só o esconde). Isso
alcançava a emissão como `(string_value_ = _syntaxBridgeUnsupported<String?
>(...))! = s;` — Dart inválido ("Illegal assignment to non-assignable
expression"), o que fez `jsonxx.dart` também ficar não-parseável na primeira
remedição desta rodada. Fix: `Expr::Assign` agora propaga o motivo do
bailout do lado direito antes de se construir, igual ao padrão já usado em
outros pontos deste módulo para operando `Unsupported`.

**Medição (as três correções juntas, uma única remedição confiável):**
tipos 2.802/241 → **2.831/242** (+29); expressões 7.339 → **7.125** (−214,
a atribuição-como-expressão liberou mais casos do que os dois bugs novos
custaram); statements 430/17 → **437/18** (+7, o novo bailout honesto do
`switch` quando a dobra falha); total 10.571 → **10.393** (−178, −1,7%).
Arquivos não-parseáveis: **1/301** (`pugixml.dart` só — aritmética de
ponteiro cru já deliberadamente adiada; `jsonxx.dart` também ficou válido de
novo, melhor que a base histórica de 2/301). `dart analyze` 14.512 → 14.369
erros (−143), 7.844 → 7.864 avisos (+20, ruído). Conferido manualmente:
zero panics (`grep "panicked at"` vazio), zero tokens `dynamic`, testes de
regressão dedicados para as três correções em `crates/server/tests/
lower_cpp.rs`.

### Atualização de 2026-08-20 (15ª rodada) — literal de `std::map`/`unordered_map`

Achado real: tabelas de consulta `static const std::map<K, V> nome{ {k1,
v1}, ... };` espalhadas pelo corpus (`midifunctor.cpp`'s `durationEq`,
`iocmme.cpp`'s `accidMap`/`stemDirMap`, entre outras) — o segundo maior
contribuinte de "unsupported expression cursor kind 119"
(`CXCursor_InitListExpr`), atrás só do padrão de inicialização agregada
como argumento/retorno (`push_back({a, b, c})`, `return {a, b, ...}`, esse
sim fora de escopo desta rodada). Diferente da lista plana de
`std::vector`/`array`/`deque`/`initializer_list` (já suportada, 2ª rodada),
cada entrada de `std::map` é sua própria lista de 2 elementos `{ chave,
valor }` — confirmado empiricamente (não assumido do AST interno completo,
que embrulha cada entrada num `CXXConstructExpr` implícito de `std::pair`
que a API de cursor mais grossa do `libclang` nunca expõe): o cursor de
cada par também é `CXCursor_InitListExpr`, com exatamente 2 filhos diretos
— achado só confirmado depois que a primeira tentativa (assumir
`CXCursor_CallExpr`/construção de `std::pair`, por analogia com `clang
-Xclang -ast-dump`) falhou no teste de TDD e expôs a discrepância entre a
API completa e a API de cursor.

Fix: novo nó de IR `Expr::MapLiteral { entries: Vec<(Expr, Expr)>, ty,
origin }`, roteado em `lower_call_expr` quando o construtor pertence a
`map`/`unordered_map` e seu argumento (depois de desembrulhar wrappers
transparentes) é um `InitListExpr` cujos filhos são todos listas de 2
elementos; cai no fallback genérico existente (bailout honesto) para
qualquer outro formato. Emitido como literal de mapa nativo do Dart
(`<K, V>{k1: v1, k2: v2}`), confirmado via `dart format`/`dart analyze`/
`dart run` reais sobre um arquivo standalone antes de aceitar como
correto. Testes de regressão cobrem `std::map`/`unordered_map`, chave
inteira e valor de constante de enum nomeada (`data_STAFFREL.STAFFREL_above`
-shaped), em `crates/server/tests/lower_cpp.rs`.

**Medição:** "unsupported expression cursor kind 119": 150 → **45**
(resíduo confirmado ser a família de inicialização agregada como
argumento/retorno, fora de escopo — próximo candidato natural, não
implementado ainda). Expressões totais 7.125 → **7.020** (−105, a família
inteira do `std::map`); tipos e statements inalterados (2.831/242,
437/18); total 10.393 → **10.288** (−105, −1,0%). Arquivos não-parseáveis:
inalterado em **1/301** (`pugixml.dart`). `dart analyze` 14.369 → 14.287
erros (−82), 7.864 → 7.864 avisos (inalterado). Conferido manualmente:
zero panics (`grep "panicked at"` vazio), zero tokens `dynamic`.

### Atualização de 2026-08-20 (16ª rodada) — constante `NULL`/`__null` (idioma pré-`nullptr`)

Achado real: `adjustaccidxfunctor.cpp:25`'s `m_currentMeasure = NULL;`
(atribuição de campo em construtor), junto com o mesmo idioma em
comparação (`measure != NULL`) e inicializador de variável local
(`Measure* none = NULL;`) — o idioma pré-`nullptr`, disseminado por todo o
corpus. `<cstddef>` define `NULL` como o builtin `__null` do GNU
(confirmado via `clang -E`, não assumido), reportado por `libclang` como um
cursor distinto, `CXCursor_GNUNullExpr`, com tipo `long` — não dobrado a um
literal inteiro simples como `0` já é.

Duas descobertas de processo nesta rodada, ambas preservadas para não
repetir o erro:

1. **A primeira tentativa de reprodução mínima (sem `#include <cstddef>`)
   deu um resultado enganoso.** Sem o include, `NULL` é um identificador
   não declarado — erro de compilação — e `clang`/`libclang` produzem uma
   recuperação de erro (`CXCursor_UnexposedExpr` com tipo `bool` e **zero**
   filhos reportados por `clang_visitChildren`, mesmo envolvendo uma
   comparação `!=` inteira) que não reflete o comportamento real do
   compilador para código válido. Isolar cada fixture com `-Xclang
   -ast-dump` real (não só a API de cursor) e comparar contra uma segunda
   fixture COM o include correto expôs a diferença antes de aceitar
   qualquer conclusão.
2. **A causa raiz real só apareceu ao reproduzir o padrão de atribuição de
   campo em construtor** (`m_currentMeasure = NULL;`), não a comparação
   isolada — com o include correto, a AST completa (`-ast-dump`) e a API de
   cursor concordam: `BinaryOperator '!=' [ImplicitCastExpr(LValueToRValue)
   DeclRefExpr, ImplicitCastExpr(NullToPointer) GNUNullExpr]`, uma forma
   perfeitamente normal. `CXCursor_GNUNullExpr` simplesmente nunca tinha
   handling próprio em `lower_expr` — caía no bailout genérico "unsupported
   expression cursor kind 123", mascarado pela mensagem mais genérica do
   wrapper de conversão implícita externo ("unsupported implicit
   conversion from Int to Nullable(Record...)"), que é a única mensagem
   que aparecia no diagnóstico.

Fix: `CXCursor_GNUNullExpr` agora lowera direto para `Expr::NullLiteral`
(não para `IntLiteral{0}` — `__null` só significa "o ponteiro nulo" em
todo contexto onde C++ o aceita, então não depende de um wrapper de
conversão externo para virar `null` no Dart). A branch existente de
"`Int` → `Nullable` via wrapper de conversão implícita" (que já cobria uma
literal `0` nua) foi estendida para também aceitar `Expr::NullLiteral`
como operando interno, cobrindo os dois idiomas (`0` nu e `NULL`/`__null`)
com a mesma regra. Teste de regressão cobre os três contextos reais
(atribuição de campo em construtor, comparação, inicializador de variável
local) em `crates/server/tests/lower_cpp.rs`.

**Medição:** "unsupported implicit conversion from Int to
Nullable(Record...)" (soma de todas as variantes por tipo de record):
~1.013 → **0** (família inteira eliminada — bem maior que os dois maiores
contribuintes isolados sugeriam, 152+114=266; o restante estava espalhado
por dezenas de tipos de record diferentes). Expressões totais 7.020 →
**6.007** (−1.013, −14,4%); causas distintas de expressão 541 → **409**
(−132); tipos e statements inalterados (2.831/242, 437/18); total 10.288
→ **9.275** (−1.013, −9,8%). Arquivos não-parseáveis: inalterado em
**1/301** (`pugixml.dart`). `dart analyze` 14.287 → 14.236 erros (−51),
7.864 → 7.913 avisos (+49, ruído). Conferido manualmente: zero panics
(`grep "panicked at"` vazio), zero tokens `dynamic`.

### Atualização de 2026-08-20 (17ª rodada) — `void*`/`const void*` viram handle nomeado, e lote de causas menores

Executado pelo loop autônomo de `docs/prompts/2026-08-20-loop-bailout.md`.
Baseline no início desta rodada (confirmado igual ao registrado acima, sem
divergência): tipos 2.831/242, expressões 6.007/409, statements 437/18 —
total 9.275; 1/301 arquivos inválidos; `dart analyze` 14.236 erros, 7.913
avisos.

Nove causas corrigidas com teste de lowering falho-antes/passa-depois em
`crates/server/tests/lower_cpp.rs`, seguindo a fase 4 ("Ponteiros, buffers e
callbacks") e resíduos das fases 1–3 da "Ordem de execução":

- **`void*`/`const void*` — o maior bailout de tipo isolado (896+253=1.149
  ocorrências, ~18% do total pendente).** A tabela da fase 4 já previa a
  direção ("`void*` → handle de domínio nomeado"); esta rodada implementa
  essa decisão. Novo tipo sintético `ir::Type::Record { usr:
  "syntax-bridge:native-handle", name: "SyntaxBridgeNativeHandle" }`
  (reaproveitando o mesmo truque que `SyntaxBridgePair`/`Str`/`Bytes` já
  usam — nenhuma variante nova de `Type` foi necessária), emitido como
  `SyntaxBridgeNativeHandle?` com uma classe de suporte documentada no
  arquivo compartilhado `syntax_bridge_support.dart` (mesmo mecanismo do
  `SyntaxBridgePair`). Diferente de `SyntaxBridgeOpaque` — que a definição
  do loop marca como "tipo sintetizado sem ligação com o tipo C++
  original" —, esta classe **é** a forma honesta de um `void*`: apenas
  identidade, nunca dereferenciado nem usado em aritmética, com contrato
  documentado na própria classe. Escopado exatamente ao pointee `void`;
  ponteiro para escalar/record ainda não representável continua
  `Unsupported`, sem mudança. Bug encontrado e corrigido no caminho:
  `nullptr` (`CXCursor_CXXNullPtrLiteralExpr`) nunca tinha handling
  próprio — só `NULL`/`__null` (`CXCursor_GNUNullExpr`) tinha, desde a 16ª
  rodada — e caía num bailout "conversion from Unsupported(std::nullptr_t)
  to Nullable(...)" sempre que `nullptr` era comparado/atribuído a um
  ponteiro sem passar por um cursor de conversão explícito; corrigido com o
  mesmo padrão do `GNUNullExpr`.
- **`std::basic_string::push_back(char)`** (122) — real em `toolkit.cpp`'s
  `option_str.push_back(...)` e `iopae.cpp`'s `paeStr.push_back(...)`.
  Vira `texto = texto + String.fromCharCode(c)`, reaproveitando a mesma
  forma de reatribuição que `append`/`+=` já usam (`char` já é `Type::Int`
  nesta IR).
- **`std::vector::resize(n[, valor])`** (86) — real em `staff.cpp`'s
  `lines.resize(count)`. `List.length =` do Dart só encolhe com segurança
  (crescer preenche com `null`, que quebra em runtime para elemento não
  anulável); vira um `if`/`else` explícito: encolher usa `list.length =`,
  crescer usa `list.addAll(List.filled(n - list.length, valorPadrao))`,
  com `default_scalar_value` (o mesmo helper de `MapIndexOrInsert`) para a
  sobrecarga de 1 argumento.
- **Operador de conversão para `bool`** (`operator bool() const`, 52) —
  mesma mecânica do `toStr` (5ª rodada), generalizada:
  `conversion_operator_dart_method_name` agora é a única fonte de verdade
  para os três pontos de despacho (declaração, `lower_call_expr`,
  `lower_method_call`), com `Bool` mapeado para `toBool`.
- **`delete ponteiro;`** (`unsupported statement cursor kind 135`, 44) —
  real em `layer.cpp`'s `delete m_staffDefClef;`, `toolkit.cpp`'s `delete
  m_editorToolkit;`. Esta IR não rastreia posse (`Owned<T>`/`dispose()`
  continua trabalho futuro), então todo ponteiro hoje é uma referência
  gerenciada pelo GC do Dart — `delete` manual já não tem efeito nenhum
  para representar; omitido como statement, igual ao `NullStmt`.
- **`struct { ... } s, *ps;` (struct/enum/union anônimo definido junto do
  declarador)** — real, causa raiz de duas famílias ao mesmo tempo:
  "`DeclStmt`'s declarator is not a `VarDecl`" (44) e "`VarDecl` had 2
  initializer-shaped children" (44). Confirmado via `clang++ -Xclang
  -ast-dump`: o cursor da definição de tipo (`CXXRecordDecl`/
  `CXCursor_StructDecl` na API de cursor) aparece duas vezes — como irmão
  do `VarDecl` dentro do `DeclStmt`, e de novo como filho de *cada*
  `VarDecl` individual (quirk de navegação do libclang, mesma categoria já
  documentada para `TypeRef`/`NamespaceRef`/`TemplateRef`). Ambos os
  pontos de filtro (`lower_multi_decl_stmt` e o `init_candidates` de
  `lower_one_var_decl`) passaram a descartar
  `StructDecl`/`ClassDecl`/`UnionDecl`/`EnumDecl` além dos três já
  filtrados.
- **`std::multiset<T>`** (9, tipo) — vira `List<T>` (preserva duplicatas,
  que `Set<T>` descartaria silenciosamente), não sua ordenação automática —
  a mesma aproximação documentada já aceita para `unordered_set`/
  `unordered_map`.
- **`for` com uma ou mais cláusulas omitidas** (`for (;;)`-shaped,
  "ForStmt had 3/1 children", 28+20=48) — item explicitamente adiado desde
  a 1ª rodada por fragilidade. Implementado por desambiguação de token:
  `for_stmt_clause_presence` tokeniza a extensão do próprio `ForStmt`,
  localiza os dois `;` de topo (profundidade de parênteses 1) e o `)` de
  fechamento, e decide quais dos três segmentos estão vazios — sem
  depender da contagem de filhos do `clang_visitChildren`, que omite uma
  cláusula ausente sem marcador posicional. Defensivo: a contagem de
  cláusulas derivada dos tokens é cruzada com a contagem real de filhos
  antes de aceitar qualquer atribuição; qualquer divergência (ex.: um `;`
  dentro de uma lambda no cabeçalho do `for`, caso extremo não observado
  no corpus real) cai no bailout já existente, sem risco de atribuir a
  cláusula errada silenciosamente.
- **`case` que cai no próximo sem `break` (fallthrough)** (36) — Dart tem
  sua própria sintaxe explícita de fallthrough (`continue <rótulo>;` para
  um `case`/`default` irmão rotulado, sintaxe real confirmada, não
  suposta). Cada `case` não-vazio sem terminador ganha um
  `Stmt::ContinueLabel` apontando para o próximo `case` em ordem de fonte,
  que ganha um `SwitchCase::label` impresso logo antes de si. A cláusula
  textualmente última (o último `case` quando não há `default`, ou o
  `default` — `emit::dart` sempre imprime `default` por último,
  independente da posição real na fonte) não precisa de terminador algum,
  a mesma regra que Dart e C++ já compartilham para o fim de um `switch`
  (achado colateral: o bailout antigo exigia terminador até para essa
  cláusula, mais estrito do que precisava). Escopo mantido: cair de um
  `case` **para dentro do** `default` continua bailout — `default` não tem
  slot de rótulo próprio (`Stmt::Switch.default` é um `Vec<Stmt>` puro, não
  um `SwitchCase`), lacuna real e mais estreita, não implementada nesta
  rodada.

**Medição:** tipos 2.831/242 → **1.730/240** (−1.101, −2 causas — quase
todo o volume de `void*`/`const void*`, com pequena exposição de cobertura
compensando); expressões 6.007/409 → **5.821/355** (−186, **−54 causas**);
statements 437/18 → **391/16** (−46, −2 causas); total de ocorrências
9.275 → **7.942** (−1.333, −14,4%); total de causas distintas 669 →
**611** (−58). Arquivos não-parseáveis: inalterado em **1/301**
(`pugixml.dart`). `dart analyze` 14.236 → 15.298 erros (+1.062), 7.913 →
8.470 avisos (+557) — mesmo padrão de exposição de cobertura já registrado
em quase toda rodada anterior (mais código alcançado, não uma piora de
qualidade). Conferido manualmente: zero panics (`grep "panicked at"`
vazio), zero tokens `dynamic` (varredura em todo `.diagnosis/dart-package`
excluindo `dynamic_cast`/`DynamicCast`), as 5 ocorrências de spelling
vazio em tipos são as mesmas de sempre (família "Spelling vazio" já
rastreada, não investigada nesta rodada).

Efeitos colaterais de cobertura confirmados manualmente, não regressão:

- **Família nova "conversion ... to/from `Nullable(SyntaxBridgeNativeHandle)`"**
  (~30 causas distintas, ~230 ocorrências somadas). Consequência direta e
  esperada de dar ao `void*` um tipo preciso pela primeira vez: conversões
  que antes caíam sob mensagens vagas de "para/de `Unsupported("void
  *")"` (ou nem eram alcançadas, escondidas atrás do bailout de tipo)
  agora aparecem nomeadas e granulares — `Bytes`↔handle, `Str`↔handle,
  `List(Int)`↔handle, `Callback` com parâmetro handle. Nenhuma delas foi
  adivinhada (nenhuma faz auto-coerção entre buffer e handle opaco); é
  exatamente a "fronteira explícita nomeada" que o AGENTS.md pede, só que
  agora com granularidade nova em vez de um bucket `SyntaxBridgeOpaque`
  único. Candidato natural da fase 4 para a próxima rodada: decidir, caso a
  caso, se cada uma dessas conversões deve virar um bridge com contrato
  (ex.: um callback ABI genuíno com parâmetro `void*` de contexto) ou
  continuar bailout.
- **`unsupported statement cursor kind 210` (`GotoStmt`)**: 11 → 42
  (+31). **`unsupported statement cursor kind 203` (`CaseStmt`), nova**:
  0 → 31. Rastreado com instrumentação temporária de debug (removida antes
  do commit): as 31 ocorrências de "cursor kind 203" vêm todas de uma
  única localização real, `include/zip/zip_file.hpp:3740` (a mesma
  contagem porque o header é incluído — e portanto relowered — em muitas
  unidades de compilação). É o idioma "dispositivo de Duff" de máquina de
  estados do miniz (macros `TINFL_CR_BEGIN`/`TINFL_CR_RETURN`): um `switch
  (r->m_state) { case 0: ... }` cujos `case state_index:` adicionais ficam
  dentro de blocos `do { ... } while (0)` aninhados, não como filhos
  diretos do `CompoundStmt` do `switch` — fora do escopo do `switch`
  lowering desta e de rodadas anteriores, que já exigia rótulos como
  filhos diretos. A correção de `for` sem cláusulas desta rodada (Verovio
  usa exatamente esse idioma de máquina de estados em loops `for (;;)`)
  provavelmente é o que passou a alcançar mais fundo esta função
  específica; nenhuma mudança desta rodada toca `switch`/`goto` dentro de
  blocos aninhados. Ambos continuam bailouts honestos, não `dynamic`, sem
  Dart inválido.

Investigado e **adiado nesta rodada**, causa raiz identificada:

- **Structured bindings** (`const auto [a, b] = par();`) — o residual de
  "`DeclStmt`'s declarator is not a `VarDecl`" (44 → 29). Achado real em
  `adjustbeamsfunctor.cpp:268`'s `const auto [above, below] =
  outerBeamInterface->GetAdditionalBeamCount();`. C++17 representa isso
  como um `DecompositionDecl` (não um `VarDecl` comum) com um
  `BindingDecl` por nome desestruturado — precisa de tradução para o
  padrão de destructuring do Dart (`final (above, below) = ...;` quando a
  fonte já é um `SyntaxBridgePair`/tuple, ou campo a campo quando é um
  record), decisão de mapeamento nova, não uma extensão do filtro desta
  rodada.
- **"`VarDecl` had 2 initializer-shaped children" residual (43, quase
  inalterado)** — o fix de struct/enum/union anônimo desta rodada não foi
  a causa dominante desta família (só moveu 1 ocorrência); a causa
  dominante continua desconhecida. Próxima rodada: instrumentar a mesma
  forma que localizou o "cursor kind 203" acima (debug temporário
  imprimindo `origin` por ocorrência) para achar o padrão real antes de
  tentar mais um filtro às cegas.

## 1. Tipos sem mapeamento — snapshot-base de 4.384 ocorrências

### Progresso executado

| Mapeamento | Representação Dart | Efeito verificado |
| --- | --- | --- |
| `std::array`, `deque`, `initializer_list` | `List<T>` | preserva a forma de coleção; o bound de `array` continua disponível no catálogo. |
| `unordered_set` / `unordered_map` | `Set<T>` / `Map<K,V>` | remove o bailout de tipo; diferenças de ordem permanecem responsabilidade das operações. |
| `optional`, `unique_ptr`, `shared_ptr`, `weak_ptr` | `T?` | representa presença/ausência sem alegar preservar ownership. |
| `char_t*` e aliases canônicos de caractere | `String?` | cobre a superfície de texto do PugiXML sem classificar byte buffer como texto. |
| `uint8_t*`, `mz_uint8*` | `Uint8List?` | introduz o tipo IR `Bytes` e a importação `dart:typed_data`. |
| `void*` com parâmetro escalar irmão `*_size`/`*_length` | `Uint8List?` | só classifica buffer quando o nome prova a relação; handles `void*` isolados continuam bailout explícito. |
| `R (*)(Args...)` com assinatura representável | `R Function(Args...)` | closure Dart tipada para callback C/C++; callbacks de ABI continuam fronteira FFI. |
| arrays nativos `T[N]` | `List<T>` | preserva elemento e forma de valor em campos; tamanho continua uma regra de fronteira futura. |
| `std::pair<A, B>` | `SyntaxBridgePair<A, B>` | adaptador compartilhado preserva `first`/`second` e identidade nominal entre arquivos. |
| `std::tuple<T...>` | record Dart `(T, ...)` | preserva slots posicionais na fronteira de tipo; `std::get` ainda precisa de lowering de expressão próprio. |
| aliases dependentes de contêiner, como `size_type` | tipo canônico (`int`, `double`, ...) | descarta `TemplateRef` de navegação do libclang antes de escolher o inicializador da variável. |

Os testes de lowering cobrem cada linha da tabela, inclusive a armadilha de
libclang que expunha o `3` de `std::array<int, 3>` como se fosse argumento
padrão do parâmetro Dart.

| Família | Qtde. | Causa raiz | Direção de solução |
| --- | ---: | --- | --- |
| Ponteiros, `void*` e callbacks | 2.654 | `lower_type` só reduz `T*` a `T?` quando `T` já é record/`String`/coleção. Ponteiros escalares, buffers, callbacks e handles caem no ramo FFI genérico. | Classificar por uso: referência de objeto → `T?`; buffer numérico → `TypedData`/`Span<T>`; ABI/callback → `Pointer`/`NativeFunction` ou `typedef` Dart; `void*` → handle de domínio nomeado. A aritmética de ponteiro continua uma fronteira FFI explícita. |
| Streams, texto e regex C++ | 483 | `basic_istream`, `basic_ostream`, `stringstream`, `regex` e iteradores de regex não têm adaptador no lowering. | `CppInput`/`CppOutput` como fronteira externa, `StringBuffer`/`String` para casos locais, `RegExp` e resultado tipado quando a semântica for equivalente. Não fingir que um stream C++ é uma `String`. |
| Produtos (`pair`/`tuple`) | 313 | Especializações `std::pair` e `std::tuple` chegam como records de template não reconhecidos. | Adaptadores recursivos `Pair<A,B>` e records Dart. Preservar `first`/`second`; tuples de aridade variável precisam de record nomeado ou classe auxiliar. |
| Arrays e buffers fixos | 192 | Arrays C/C++ não têm variante IR; aparecem `Point[4]`, `mz_uint8[ ]`, arrays multidimensionais. | `FixedArray<T,N>`/`List<T>` para valor; `Uint8List`/`Uint16List` etc. para buffer; arrays multidimensionais como wrapper de *stride*. Nunca reduzir um buffer a ponteiro opaco sem preservar tamanho. |
| Coleções, iteradores e adaptadores STL | 147 | Há suporte parcial para `vector`/`list`/`set`/`map`; `deque`, `stack`, `multiset`, iteradores e `initializer_list` ainda são desconhecidos. | Mapear para `List`/`Set`/`Map`, `Queue`, `Iterable`/cursor tipado e `MultiMap`; reter adaptador quando ordem, multiplicidade ou invalidação de iterador for relevante. |
| Escalares e aliases que escapam | 188 | Tipos dependentes/aliases (`size_type`, `value_type`, `int`, `double`, `void`) chegam como `Unexposed`/typedef e não são sempre des-açucarados até o tipo canônico. | Aplicar `clang_getCanonicalType`/des-açucaramento antes do fallback, mantendo a informação de largura/sinal no catálogo. Inteiros → `int`; ponto flutuante → `double`; `void` só é válido como retorno, nunca valor. |
| Lambdas | 93 | O tipo da lambda não tem nome Dart e seu cursor ainda não é convertido em fechamento. | Closure Dart quando captura e assinatura puderem ser lowered; `typedef`/ponte FFI para callback ABI; caso contrário, fronteira externa nomeada com origem e contrato. |
| Unions e tipos anônimos | 57 | União e enum/struct anônimos não têm representação nominal segura; alguns spellings incluem a localização do header. | União de domínio → tipo discriminado/`sealed`; união de layout → `Struct` FFI; tipo anônimo só ganha nome sintetizado estável se todos os campos forem representáveis. |
| Opcional, posse e nulidade | 19 | `optional`, `unique_ptr` e `nullptr_t` ainda não têm lowering recursivo. | `T?` para `optional`; `Owned<T>`/handle com `dispose` para posse; `null` para `nullptr`. Decidir transferência de propriedade antes de emitir. |
| Spelling vazio | 171 | A declaração/type canônico não foi resolvido e o fallback perdeu o nome. | Nunca emitir ponte sem identidade: registrar cursor, USR, tipo canônico e localização. Resolver como alias/externo ou falhar o diagnóstico com causa rastreável. |
| Outros tipos nomeados | 67 | Bibliotecas e aliases específicos (`tm`, `Scale`, `KeyboardMapping`, `basic_fstream` etc.). | Classificar no catálogo como record do projeto, adaptador de biblioteca ou API externa. Cada spelling novo deve ter decisão registrada antes de sair do diagnóstico. |

## 2. Expressões sem lowering — snapshot-base de 27.905 ocorrências

| Família | Qtde. | Causa raiz | Direção de solução |
| --- | ---: | --- | --- |
| Conversões implícitas | 12.689 | O wrapper de conversão só aceita tipos idênticos e `int → double`; C++ usa verdade de inteiro/ponteiro, enums, upcasts e conversões numéricas. | Criar IR de conversão explícita: `int → bool` (`!= 0`), ponteiro → bool (`!= null`), `bool → int` (`? 1 : 0`), enum ↔ valor subjacente, upcast seguro pela hierarquia e conversão numérica validada. Downcast só com cast C++ explícito ou prova de subtipo; nunca inventar `as` para uma conversão insegura. |
| Chamadas STL não mapeadas | 3.582 | O dono da chamada é reconhecido, mas o método não possui regra (`string.find`, `vector.push_back`, `at`, `c_str`, `map`, `list`, streams etc.). | Tabela de adaptadores por coleção/string/stream. Prioridade: `basic_string` e `vector`; depois `list`/`map`/`stack`; operações de byte (`c_str`, `find`, `compare`) precisam preservar a semântica C++ ou ir para ponte externa. |
| Forma de chamada STL inesperada | 1.783 | O lowering exige `MemberRefExpr` como primeiro filho; wrappers, conversões e chamadas dependentes quebram essa suposição. | Normalizar receptor e argumentos antes de despachar o método STL; extrair a chamada de `UnexposedExpr`/casts e só então aplicar a tabela de adaptadores. |
| Operadores unários | 3.744 | Só o menos unário é lowered. O corpus contém `!` (2.153), `++`/`--`, endereço e dereferência. | `!` → `!` após política de verdade; `++`/`--` → atribuição com semântica pré/pós preservada; `&`/`*` somente através da ponte de ponteiros/buffers. |
| Operadores binários | 989 | Faltam `||`, atribuição como expressão, bitwise, deslocamentos e vírgula. | Para inteiros, mapear bitwise/deslocamentos para operadores Dart; `||` direto; reescrever atribuição encadeada em statements/temporários; preservar ordem de avaliação para vírgula. |
| Chamadas de operadores | 1.738 | Sobrecargas como `operator=`, `operator<<` e `operator[]` não passam pelo mapeamento de métodos Dart. | Despachar por semântica: cópia/atribuição, índice, comparação, fluxo de saída. Métodos sem equivalente direto recebem adaptador nomeado, não bailout opaco. |
| Cursor de expressão ainda não tratado | 2.640 | Há cursores C++ com significado conhecido, mas sem variante IR. | Resolver a tabela detalhada abaixo; qualquer cursor novo deve entrar no relatório com seu nome Clang, não apenas o número. |
| Valor padrão de campo | 589 | `default_record_construct` não conhece valor seguro para vários tipos. | Política de inicialização por tipo: construtor obrigatório, `late`, `null` apenas para anuláveis, factory/zero somente quando semanticamente definido. |
| `VarDecl` com forma composta | 410 | O lowering aceita no máximo um inicializador-filho. | Separar declaradores e inicializadores em múltiplos `Stmt::VarDecl`, preservando a ordem e efeitos colaterais. |
| Alvo de chamada não reduzido | 168 | `lower_call_expr` aceita apenas função livre, método e construtor; chamadas via campo/variável/callback/conversion operator ficam de fora. | Resolver um `Callable` unificado: `FunctionDecl`, `CXXMethod`, `Constructor`, `ConversionFunction` e valores chamáveis (campo/parâmetro com tipo de função). |
| Referência de membro com filhos extras | 95 | A forma do AST traz qualificadores/receptores implícitos além dos 0/1 filhos esperados. | Normalizar `MemberRefExpr`, descartando referências de tipo/template e preservando o único receptor de valor. |
| Wrapper/caso residual | 3 | Wrappers sem filho de valor ou alvo de chamada irresolúvel. | Capturar árvore e token fonte no diagnóstico; adicionar normalização específica ou rejeitar como fronteira externa. |

### Cursores de expressão ainda sem variante IR — 2.640 ocorrências

| Cursor Clang | Qtde. | Solução |
| --- | ---: | --- |
| `CXXNewExpr` (134) | 617 | Criar record Dart quando a alocação é de tipo representável; caso dono/ponteiro seja relevante, criar `Owned<T>` ou usar FFI. |
| `CharacterLiteral` (110) | 496 | Lower para `int` code unit; converter para `String` somente em contexto textual explícito. |
| `ConditionalOperator` (116) | 471 | Nova expressão ternária Dart `cond ? a : b`, com unificação de tipos. |
| `ArraySubscriptExpr` (113) | 215 | Reusar `Expr::Index`; a escrita correspondente vai para atribuição indexada. |
| `TypeRef` (43) / `TemplateRef` (45) | 363 | Não são valores: removê-los da normalização de filhos de expressões/chamadas. |
| `UnaryExpr` (136) | 178 | Distinguir `sizeof`/`alignof`/extensões; mapear apenas quando o tamanho Dart/FFI estiver definido. |
| `CXXDynamicCastExpr` (125) | 169 | Cast verificado: `is` + `as T`/`T?`, condicionado à hierarquia lowered. |
| `LambdaExpr` (144) | 51 | Closure Dart ou callback FFI, conforme a captura e ABI. |
| `CXXThrowExpr` (133) | 33 | `Stmt::Throw`/expressão `throw`, preservando o objeto de exceção. |
| `CXXReinterpretCastExpr` (126) / `CXXConstCastExpr` (127) | 23 | Apenas adaptador FFI seguro ou remoção de const quando não altera representação; nunca cast Dart inventado. |
| `CompoundAssignOperator` (115) | 10 | Reusar lowering de atribuição composta, inclusive alvo indexado/campo. |
| `InitListExpr` (119) | 7 | `List`/`Set`/record literal tipado conforme o destino. |
| `ConceptSpecializationExpr` (153) / `DeclStmt` vazando como expressão (231) | 7 | Tratar conceito como informação de tipo e elevar declaração para statement. |

## 3. Statements sem lowering — snapshot-base de 2.214 ocorrências

| Família | Qtde. | Causa raiz | Direção de solução |
| --- | ---: | --- | --- |
| `continue` | 469 | `CXCursor_ContinueStmt` (212) não tem variante IR. | Adicionar `Stmt::Continue` → `continue;`. |
| Atribuição indexada | 417 | Só variável simples e campo são alvos de atribuição. | Generalizar alvo para expressão atribuível (`Index`, `FieldAccess`, `Ref`) e emitir `target = value`. |
| `for` baseado em intervalo | 320 | `CXXForRangeStmt` (225) não é lowered. | `for (final item in iterable)`; quando houver mutação/referência, usar cursor/adaptador explícito. |
| `break` | 297 | `CXCursor_BreakStmt` (213) não tem variante IR. | Adicionar `Stmt::Break` → `break;`. |
| `switch` | 141 | `CXCursor_SwitchStmt` (206) sem representação. | IR de `switch`/`case`/`default`, com `break` e queda controlada explicitamente. |
| Declarações múltiplas/não-variável | 212 | `DeclStmt` admite vários declaradores e alguns não são `VarDecl`. | Desdobrar em statements ordenados; tratar declarações de tipo/using separadamente. |
| Statement vazio/bloco | 147 | `NullStmt` (230) e `CompoundStmt` (202) chegam sem regra. | Omitir vazio; achatar bloco mantendo escopo quando houver declaração/RAII. |
| `do…while` | 56 | `CXCursor_DoStmt` (208) sem IR. | Emitir `do { ... } while (cond);` em Dart. |
| Atribuição composta | 60 | Operadores `<<=`, `>>=`, `&=`, `|=` e alvos não simples não são tratados. | Reusar alvo atribuível e operadores Dart equivalentes; para índice/buffer, usar adaptador de escrita. |
| `for`/`if` com forma parcial | 44 | O lowering assume conjunto fixo de filhos; C++ permite cláusulas omitidas. | Representar `init`/`condition`/`increment` opcionalmente (o IR já comporta isso) e aceitar as formas AST válidas. |
| `delete`, `goto`, label | 47 | `CXXDeleteExpr` (135), `GotoStmt` (210) e `LabelStmt` (201) chegam como statement não suportado. | `delete` vira `dispose` somente para `Owned<T>`; `goto`/labels exigem reestruturação de fluxo ou fronteira externa — não há equivalente Dart direto. |
| Exceções C++ múltiplas/catch-all | 2 | `CXXTryStmt`/`CXXCatchStmt` aceitam mais formas que o IR atual. | Vários `catch` Dart e `catch (_, stack)` para catch-all, com tradução de tipos de exceção. |
| Declaração sem definição | 2 | Função externamente declarada sem corpo do projeto. | Marcar como externo e gerar mock/ponte pelo fluxo de externos; não emitir statement de bailout. |

## Ordem de execução

1. **Conversões + controle de fluxo barato.** Verdade C++, upcast seguro,
   enum/inteiro, `continue`, `break`, statement vazio, `do…while` e range-for
   removem uma fração grande sem dependência de bibliotecas externas.
2. **Normalização de AST.** Remover `TypeRef`/`TemplateRef` dos filhos de
   valor, aceitar cláusulas opcionais e normalizar receptores STL. Isso reduz
   bailouts sem decidir semântica de domínio.
3. **Adaptadores de `String` e coleções.** Implementar a tabela de métodos
   de maior volume com testes de equivalência, distinguindo APIs por byte de
   APIs por code unit.
4. **Ponteiros, buffers e callbacks.** Usar os fatos do catálogo para escolher
   referência, buffer ou FFI. Esta etapa não pode ser substituída por cast
   dinâmico/ponteiro opaco genérico.
5. **Produtos, streams, regex, ownership e tipos anônimos.** Cada um ganha
   adaptador reutilizável ou decisão explícita de fronteira externa.
6. **Casos sem equivalente (`goto`, `reinterpret_cast`, união de layout).**
   Exigir ponte externa/FFI ou uma transformação de controle comprovada; o
   bailout permanece até existir essa decisão, mas é rastreado por causa.

## Regra de regressão

`just verovio-diagnosis` agora serializa as três tabelas completas no campo
`bailouts` do snapshot. Antes de reduzir uma família, acrescente uma fixture
mínima e um teste que falhe; depois compare a contagem daquela razão no
Verovio. Uma nova razão, spelling vazio ou retorno de `dynamic` é regressão de
diagnóstico, não ruído aceitável.
