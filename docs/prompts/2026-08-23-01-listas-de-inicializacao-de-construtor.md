# Tarefa 01 — Listas de inicialização de construtor são descartadas inteiras

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório; `dynamic` proibido; quando não houver
equivalente direto em Dart, a resposta é uma fronteira/adaptador nomeado e
explícito, nunca um apagamento).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/estado-da-transpilacao-verovio-6.2.md`,
família **T1**. Este prompt é autocontido.

**Esta é a tarefa mais grave do lote e deve ser feita primeiro.** Ela não
elimina muitos erros do `dart analyze` — ela conserta código que hoje sai
silenciosamente errado.

## A causa raiz

Em C++, um construtor pode inicializar campos e bases **antes** do corpo, na
lista de inicialização:

```cpp
Point(int xx, int yy) : x(xx), y(yy) {}
AdjustArpegFunctor(Doc *doc) : DocFunctor(doc) { m_currentAlignment = NULL; }
```

`lower::cpp::lower_constructor` (`crates/server/src/lower/cpp.rs:1108`) lê
**apenas** o corpo:

```rust
let body_cursor = unsafe { find_compound_stmt_child(cursor) };
```

e `ir::Constructor` (`crates/server/src/ir/mod.rs`, por volta da linha 886) não
tem campo nenhum para inicializadores. A lista some sem deixar rastro: sem
bailout, sem `TODO`, sem diagnóstico.

Há um comentário em `crates/server/src/lower/cpp.rs:1480-1486` afirmando que
`CXCursor_CXXCtorInitializer` "não existe na API pública do libclang". **A
afirmação é verdadeira e a conclusão é errada.** O `libclang` não tem um *kind*
de cursor chamado `CXXCtorInitializer`, mas expõe cada inicializador **escrito**
como filho direto do cursor do construtor, antes do `CompoundStmt`:

| Forma em C++ | Filhos que `clang_visitChildren` entrega, em ordem |
| --- | --- |
| `: x(xx)` | `CXCursor_MemberRef` (referencia o campo `x`) → cursor da expressão `xx` |
| `: Base(a, b)` | `CXCursor_TypeRef` (referencia `Base`) → cursor da expressão de construção |

Só inicializadores **escritos** aparecem (o `libclang` filtra por
`isWritten()`), que é exatamente o conjunto que interessa.

> **Confirme isso empiricamente antes de construir em cima.** O repositório tem
> a disciplina de escrever "confirmado empiricamente, não assumido" nos
> comentários; siga-a. O jeito mais barato: um teste temporário que faz o
> lowering de `struct P { int x; P(int a) : x(a) {} };` e imprime
> `clang_getCursorKind` + `clang_getCursorSpelling` de cada filho do cursor do
> construtor. Se a ordem/forma dos filhos for diferente do que está na tabela
> acima, ajuste o plano ao que você observar — e **registre o que observou** no
> comentário do código.

## A evidência

`include/vrv/devicecontextbase.h:207-208` (Verovio 6.2.0 real):

```cpp
Point() : x(0), y(0) {}
Point(int xx, int yy) : x(xx), y(yy) {}
```

`.diagnosis/dart-package/lib/devicecontextbase.dart:280-287`:

```dart
Point() {
}

Point.ctor2(int xx, int yy) {
}
```

Todo `Point(a, b)` do Verovio devolve `(0, 0)`. `Point` é o tipo de coordenada
de todo o motor de layout.

`src/pugi/pugixml.cpp:5597`:

```cpp
PUGI_IMPL_FN xml_node::xml_node(xml_node_struct* p): _root(p) { }
```

`.diagnosis/dart-package/lib/pugixml.dart:493`:

```dart
xml_node.ctor2(xml_node_struct? p) {
}
```

`root` nunca é atribuído — todo nó XML construído a partir de um ponteiro sai
vazio. O parser de MEI inteiro depende disso.

Uma varredura do pacote emitido encontra **60 construtores com parâmetros e
corpo completamente vazio** (21 em `pugixml.dart`, 4 em `devicecontextbase.dart`,
e o resto espalhado). Esses são só os casos em que a lista era *todo* o
construtor; um construtor com lista **e** corpo perde só a lista e sai
parecendo plausível. No C++ de origem há **724 linhas** com a forma
`) : Nome(`.

O único sintoma visível no `dart analyze` são os **105
`implicit_super_initializer_missing_arguments`** — 61 deles apontando para
`DocFunctor`:

```
adjustarpegfunctor.dart:16:3 — The implicitly invoked unnamed constructor from
'DocFunctor' has required parameters.
```

Isso é o `: DocFunctor(doc)` que deveria ter virado `super(doc)`.

## Onde mexer

1. **`crates/server/src/ir/mod.rs`** — dar a `ir::Constructor` um campo novo
   para os inicializadores. A forma mínima que serve aos dois casos:

   ```rust
   pub enum ConstructorInit {
       /// `: x(expr)` — inicializa o campo `name` do próprio registro.
       Field { name: String, value: Expr },
       /// `: Base(args)` — chama o construtor da base.
       Base { usr: String, name: String, args: Vec<Expr> },
   }
   ```

   `Constructor { …, pub inits: Vec<ConstructorInit> }`, na ordem de
   declaração. Documente o campo com o mesmo nível de detalhe dos vizinhos.

2. **`crates/server/src/lower/cpp.rs:1108` (`lower_constructor`)** — percorrer os
   filhos do cursor **antes** do `CompoundStmt` e preencher `inits`. Um
   `MemberRef` seguido de uma expressão é um `Field` (o nome do campo tem de
   passar por `dart_member_name`, o mesmo caminho que a declaração do campo usa,
   para os dois nunca discordarem). Um `TypeRef` seguido de uma expressão é um
   `Base`.

   Corrija também o comentário de `cpp.rs:1480-1486`, que hoje afirma que este
   dado é inalcançável.

3. **`crates/server/src/function_catalog.rs`** — `merge_constructors` (por volta
   da linha 584) compara `body` para detectar divergência entre partials; ele
   precisa considerar `inits` também, senão duas cópias do mesmo construtor com
   listas diferentes passam calado.

4. **`crates/server/src/emit/dart.rs:1504` (`emit_constructor`)** — emitir os
   inicializadores. Dart tem a construção equivalente:

   ```dart
   Point.ctor2(int xx, int yy) : x = xx, y = yy {
   }

   AdjustArpegFunctor(Doc? doc) : super(doc) {
     _m_currentAlignment = null;
   }
   ```

   Regras do Dart que a emissão precisa respeitar:
   - `super(...)` vem **por último** na lista de inicialização;
   - só há **um** `super(...)`;
   - uma expressão de inicializador não pode ler `this` — se o `value` de um
     `Field` referenciar outro campo do mesmo objeto, ele não cabe na lista e
     tem de descer para a primeira linha do corpo (uma atribuição comum). Essa
     é a única transformação que muda a ordem observável, e ela é segura porque
     o campo já tem um valor default na declaração;
   - se o registro for emitido como `mixin` (a decisão global da tarefa 02 do
     lote anterior), ele não pode ter construtor nenhum, e o `super(...)` de um
     mixin não existe. Nesse caso o inicializador de base tem de virar uma
     chamada explícita ao método de inicialização correspondente, ou um bailout
     honesto — nunca ser descartado em silêncio.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro**, no estilo de `crates/server/tests/lower_cpp.rs`
   (fixture pequeno num workspace temporário, `libclang` de verdade, sem mock):

   ```cpp
   struct Ponto {
       int x;
       int y;
       Ponto(int a, int b) : x(a), y(b) {}
   };
   int usa() { Ponto p(3, 4); return p.x + p.y; }
   ```

   Verifique no IR que o `Constructor` de `Ponto` tem dois `ConstructorInit::Field`
   com os valores certos. Depois, no estilo de `crates/server/tests/emit_dart.rs`,
   verifique que o Dart emitido contém `: x = a, y = b`.

2. **Teste de base**: uma classe `Derivada` cujo construtor faz
   `: Base(v)`. Verifique `ConstructorInit::Base` no IR e `: super(v)` no Dart.

3. **Teste do caso que precisa descer para o corpo**: `: x(a), y(x)`. Verifique
   que o segundo vira atribuição no corpo e que o Dart resultante analisa.

4. Implemente até passar.

5. Rode a suíte inteira: `just test` (ou `just test-host`, registrando no
   resumo), `just check`, `just lint`. Os exemplos `examples/E04` e `E06` já
   têm construtores e oráculo comportamental — eles são a rede de segurança
   real; se algum golden mudar, revise o diff antes de `just examples-bless`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- **A métrica principal não é uma contagem de erro, é uma verificação direta.**
  Em `.diagnosis/dart-package/lib/devicecontextbase.dart`, o construtor
  `Point.ctor2` precisa atribuir `x` e `y`. Em `pugixml.dart`, `xml_node.ctor2`
  precisa atribuir `root`. Verifique os dois à mão e cite-os no resumo.
- Construtores com parâmetros e corpo vazio: hoje **60** no pacote emitido.
  Devem cair para perto de zero (um construtor C++ genuinamente vazio, sem
  lista, continua vazio — esses são poucos).
- `implicit_super_initializer_missing_arguments`: **105 → perto de 0**.
- `dart analyze`: nenhum `code` novo. É esperado que `invalid_assignment` e
  `argument_type_not_assignable` **subam** um pouco: expressões que antes eram
  descartadas passam a ser emitidas e a ser tipadas. Registre o antes/depois.
- Nenhuma das três contagens de bailout de `.diagnosis/verovio-6.2.0.md` deve
  subir mais que ~2% (um inicializador cuja expressão não tem lowering vira
  bailout, e isso é honesto — mas se subir muito, o filtro está errado).

## Quando parar e perguntar

Só por decisão de **produto**. Um caso previsível: um registro emitido como
`mixin` cuja lista de inicialização chama o construtor da base. Dart não
permite construtor em `mixin`, então a tradução exige escolher entre (a)
transformar o inicializador de base numa chamada a um método de inicialização
gerado, ou (b) forçar esse registro a ser `class`, revisando a decisão global da
tarefa 02 do lote anterior. Isso muda a forma do Dart gerado de modo
observável — pergunte, com a contagem de quantos registros caem nesse caso.

Dificuldade técnica não é motivo para parar. Se o `libclang` não entregar os
filhos na forma descrita acima, descubra a forma real com um teste e siga por
ela.
