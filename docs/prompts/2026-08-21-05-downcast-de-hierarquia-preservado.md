# Tarefa 05 — `static_cast` para baixo na hierarquia não pode ser apagado

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido, mapeamento de tipos é o
objetivo central do produto).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F7**. Este prompt é autocontido.

## A causa raiz

`lower::cpp::is_transparent_wrapper` (`crates/server/src/lower/cpp.rs`, por
volta da linha 4542) trata uma lista de cursores como invólucros que podem ser
desembrulhados sem perder informação:

```rust
matches!(kind,
    CXCursor_UnexposedExpr
        | CXCursor_ParenExpr
        | CXCursor_CXXFunctionalCastExpr
        | CXCursor_CXXStaticCastExpr
        | CXCursor_CStyleCastExpr)
```

O doc comment justifica incluir `CXXStaticCastExpr`/`CStyleCastExpr`: para
conversões numéricas (`static_cast<double>(m_numerator)`), a comparação de tipo
externo/filho logo abaixo já produz um `Expr::Convert`, então desembrulhar não
perde nada.

Isso vale para números. **Não vale para um downcast de ponteiro.** Quando o
cast é `Object*` → `Doc*`, os dois lados são `Type::Nullable(Record)` com
registros *diferentes*; nenhum caminho de `Expr::Convert` reconhece esse par, e
o operando é passado adiante carregando o tipo da **base**. O downcast some.

Repare que `dynamic_cast` **é** tratado, e bem: `lower::cpp::lower_dynamic_cast_expr`
(mesmo arquivo, por volta de 5049) emite `x is T ? x : null`, com um guard que
só aceita operando simples (`this` ou uma referência a local/parâmetro), para
não reavaliar uma chamada duas vezes. O Verovio, porém, quase nunca escreve
`dynamic_cast` direto: escreve `vrv_cast`, que é
`#define vrv_cast static_cast` em build de release
(`include/vrv/vrvdef.h:65`).

## A evidência

C++ original, `src/iomei.cpp:363`:

```cpp
this->WriteDoc(vrv_cast<Doc *>(object));
```

Dart emitido, `.diagnosis/dart-package/lib/iomei.dart:348`:

```dart
WriteDoc(object);
// ← The argument type 'VrvObject' can't be assigned to the parameter type 'Doc?'.
```

O mesmo bloco (`iomei.dart:343-390`) repete o padrão para `Mdiv?`, `Pages?`,
`VrvScore?`, `Page?`, `System?` — cada `vrv_cast` do `WriteObjectInternal`
virou um argumento do tipo da base.

Outro: `.diagnosis/dart-package/lib/calcstemfunctor.dart:474` →
`note = parent;` (um `LayerElement` numa variável `VrvNote?`).

`dart analyze` (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791 diagnósticos)
atribui a esta família ~**1.523** ocorrências:

| `code` | n atribuídos | exemplo de mensagem |
| --- | ---: | --- |
| `argument_type_not_assignable` | 1238 | `The argument type 'VrvObject' can't be assigned to the parameter type 'Doc?'.` |
| `invalid_assignment` | 269 | `A value of type 'VrvObject?' can't be assigned to a variable of type 'VrvNote?'.` |
| `return_of_invalid_type` | 16 | `A value of type 'App' can't be returned from the method 'Clone' because it has a return type of 'VrvObject?'.` |

Concentração de `argument_type_not_assignable`: `iohumdrum.dart` (577),
`editortoolkit_neume.dart` (233), `iomei.dart` (230).

Atenção: nem todos os 1.569 `argument_type_not_assignable` do relatório são
desta família — 313 vêm de `SyntaxBridgeOpaque` (tarefa 06) e 18 de aritmética
`int`/`double` (tarefa 11).

Os `return_of_invalid_type` de `Clone()` têm uma nuance: `A value of type 'Fb'
can't be returned … return type of 'VrvObject?'` acontece porque `VrvObject` é
emitido como `mixin` e `Fb` não o aplica visivelmente — parte disso é da tarefa
02, não desta. Meça depois de 01 e 02 antes de concluir.

## Onde mexer

- `crates/server/src/lower/cpp.rs`:
  - `is_transparent_wrapper` (~4542) e o caminho de `lower_expr` que a consome
    (~4559) — o lugar onde o cast é desembrulhado.
  - `lower_dynamic_cast_expr` (~5049) — a forma que já existe para o cast
    checado, e o guard de operando simples que dá para reaproveitar.

A direção: reconhecer o caso em que um `CXXStaticCastExpr`/`CStyleCastExpr`
muda o *registro* de um ponteiro, e emitir um downcast Dart real em vez de
desembrulhar.

Diferença importante em relação a `dynamic_cast`: `static_cast` em C++ é **não
checado** — se o objeto não for do tipo alvo, o comportamento é indefinido, não
um ponteiro nulo. A tradução honesta é `as T?` (que estoura em tempo de
execução com um `TypeError` legível se estiver errado), **não** `is T ? x : null`
(que silenciosamente transforma um erro do programa original num nulo). Emitir
nulo aqui esconderia um bug, que é exatamente o que `AGENTS.md` proíbe.

Um `as T?` não precisa do guard de operando simples que `lower_dynamic_cast_expr`
usa (ele existe porque `x is T ? x : null` avalia `x` duas vezes), então esta
correção pode cobrir mais casos que aquela — inclusive `vrv_cast<Doc *>(f())`.

Upcast (para a base) continua sendo transparente e não deve emitir nada. Cast
entre tipos numéricos continua exatamente como está.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro. Fixture mínimo: `struct Base {}; struct Derivada :
   Base {};` e uma função `void f(Derivada*)` chamada com
   `f(static_cast<Derivada*>(b))`. Verifique que o Dart emitido tem um downcast
   e não passa o `Base` cru. Veja `crates/server/tests/lower_cpp.rs` para o
   padrão de fixture.
2. Testes de não-regressão: `static_cast<double>(i)` continua virando a
   conversão numérica que já vira hoje; um cast para a **base** (upcast)
   continua sem emitir nada; `dynamic_cast` continua produzindo a forma checada.
3. Implemente até passar.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `argument_type_not_assignable` — de **1569** para perto de 330 (o resíduo
  esperado é `SyntaxBridgeOpaque`, tarefa 06, e `int`/`double`, tarefa 11).
- `invalid_assignment` — queda de ~269.
- `return_of_invalid_type` — queda parcial; o resto depende das tarefas 01 e 02.
- Nenhum `code` novo. Em particular, `unnecessary_cast` seria sinal de estar
  emitindo `as T` onde o tipo já era `T` — barulho, corrija se aparecer.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível: `as T?` estoura em tempo de
execução quando o cast do C++ estava errado; o C++ original teria seguido com
memória mal interpretada. Se o usuário preferir uma fronteira que **reporte** o
erro em vez de estourar (um adaptador nomeado que registra e devolve nulo, por
exemplo), isso é decisão dele — mas note que `AGENTS.md` já pende para "falhar
explicitamente", então `as T?` é a recomendação. Pergunte apenas se surgir uma
alternativa igualmente defensável.

Dificuldade técnica não é motivo para parar.
