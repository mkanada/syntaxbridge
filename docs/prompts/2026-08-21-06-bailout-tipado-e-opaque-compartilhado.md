# Tarefa 06 — Bailout tipado: `SyntaxBridgeOpaque` único e sem vazar como valor

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar.
Ele é normativo, e um trecho vale citar aqui inteiro:

> **`dynamic` não é uma solução de transpilar.** […] `Type::Unsupported` é um
> diagnóstico temporário a eliminar, nao um tipo de saida aceitavel. O mesmo
> vale para bailouts de expressao: eles devem preservar o tipo estatico esperado
> e falhar explicitamente, sem propagar `dynamic`.

Esta tarefa é sobre bailouts que **não** estão cumprindo essa regra.

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F9**. Este prompt é autocontido.

## As três causas raiz (mesma origem)

### (a) `SyntaxBridgeOpaque` é redeclarado em cada arquivo

`emit::dart` define `OPAQUE_TYPE_NAME: &str = "SyntaxBridgeOpaque"`
(`crates/server/src/emit/dart.rs:26`) e emite a declaração **dentro de cada
arquivo que precisa dela**. No pacote do Verovio 6.2.0 isso dá **77 declarações
distintas**:

```
$ grep -rc "final class SyntaxBridgeOpaque" .diagnosis/dart-package/lib/ | ...
77 arquivos
```

Para o Dart, cada uma é uma classe diferente. Daí os 6 `invalid_override`, com
mensagens absurdas à primeira vista:

```
'BBoxDeviceContext.DrawSpline' ('void Function(int, SyntaxBridgeOpaque)')
isn't a valid override of 'DeviceContext.DrawSpline' ('void Function(int, SyntaxBridgeOpaque)')
```

— porque um `SyntaxBridgeOpaque` está definido em `bboxdevicecontext.dart` e o
outro em `devicecontext.dart`.

O pacote **já tem** o lugar certo: `.diagnosis/dart-package/lib/syntax_bridge_support.dart`
(`SUPPORT_FILE_NAME` em `emit/dart.rs:30`) contém `SyntaxBridgePair` e
`SyntaxBridgeNativeHandle`. `SyntaxBridgeOpaque` ficou de fora.

### (b) O valor opaco é usado como se fosse do tipo real

`_syntaxBridgeUnsupported<SyntaxBridgeOpaque>(…)` aparece em posições onde o
contexto exige um tipo concreto. Exemplos reais:

```dart
// .diagnosis/dart-package/lib/pugixml.dart:110
List<int> condition_failed = _syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…');

// .diagnosis/dart-package/lib/chord.dart:283
if (_syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…') || …) {

// .diagnosis/dart-package/lib/zip_file.dart:595
… .m_pWrite = …    // sobre um SyntaxBridgeOpaque
```

O IR já tem a representação certa: `ir::Expr::UnsupportedTyped`
(`crates/server/src/ir/mod.rs`, por volta de 446), que — diferente do
`Expr::Unsupported` legado — preserva o tipo. Há caminhos de lowering ainda
produzindo a forma sem tipo, e ela sai como `SyntaxBridgeOpaque` cru.

### (c) Bailout de expressão em posição de *padrão*

`.diagnosis/dart-package/lib/iomei.dart:3271` produz **três** erros na mesma
linha (`not_a_type`, `positional_field_in_object_pattern`,
`refutable_pattern_in_irrefutable_context`), porque um
`_syntaxBridgeUnsupported<…>(…)` caiu onde a gramática do Dart espera um
*tipo*, numa desestruturação. Isso não é só um erro de tipo: é sintaticamente
impossível.

O mesmo em `jsonxx.dart`, e a variante em closure em `iopae.dart:2071`
(`return_of_invalid_type_from_closure`).

## A evidência agregada

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791
diagnósticos) atribui a esta família ~**1.276**:

| `code` | n atribuídos | causa |
| --- | ---: | --- |
| `unchecked_use_of_nullable_value` | 334 | (b) |
| `argument_type_not_assignable` | 313 | (b) |
| `invalid_assignment` | 160 | (b) |
| `undefined_method` | 159 | (b) — `unsupportedOperator` sobre opaco |
| `non_bool_condition` | 46 | (b) — bailout numa condição |
| `undefined_getter` | 34 | (b) |
| `non_bool_negation_expression` | 29 | (b) |
| `constant_pattern_never_matches_value_type` | 21 | (b) — `switch` sobre opaco |
| `unnecessary_null_comparison` | 18 | (b) |
| `undefined_setter` | 15 | (b) |
| `non_bool_operand` | 9 | (b) |
| `invalid_override` | 6 | **(a)** |
| `not_a_type` / `positional_field_in_object_pattern` / `refutable_pattern_in_irrefutable_context` | 3 cada | **(c)** |
| `pattern_type_mismatch_in_irrefutable_context` | 2 | (c) |
| `return_of_invalid_type_from_closure` | 2 | (c) |
| `main_first_positional_parameter_type` | 1 | (b) |

## Onde mexer

- **(a)** `crates/server/src/emit/dart.rs` — `OPAQUE_TYPE_NAME` (~26),
  `SUPPORT_FILE_NAME` (~30), `emit_file` (~390) e o cálculo de imports por
  arquivo (por volta de 530-560, onde as dependências de um arquivo são
  reunidas). Mover a declaração para o arquivo de suporte e adicionar o import.
  Correção mecânica.
- **(b)** `crates/server/src/lower/cpp.rs` — auditar os sítios que constroem
  `ir::Expr::Unsupported { … }` em posição de **valor** e convertê-los para
  `UnsupportedTyped` com o tipo estático que o contexto exige. Há dezenas
  desses sítios; priorize pelos que aparecem no corpus (as mensagens de
  `reason` no Dart emitido dizem exatamente qual construção C++ disparou cada
  um — grepe `.diagnosis/dart-package/lib/` por `unsupported ` para o
  inventário).
- **(c)** `crates/server/src/lower/cpp.rs` + `crates/server/src/emit/dart.rs` —
  em posição de padrão, `on ... catch` ou `case`, um bailout de expressão não é
  representável. O bailout precisa **subir de nível**: virar
  `ir::Stmt::Unsupported` substituindo o statement/bloco inteiro, que já é uma
  forma emitível (`// TODO(syntax-bridge): …` + `throw UnimplementedError(…)`).

## Método

TDD, conforme `AGENTS.md`. Trate (a), (b) e (c) como três incrementos, cada um
com o seu teste que falha primeiro:

1. **(a)** Teste: um módulo com dois registros em arquivos diferentes, ambos
   com um membro que cai no tipo opaco, e um override entre eles. Verifique que
   `SyntaxBridgeOpaque` aparece **uma** vez, no arquivo de suporte, e que os
   dois arquivos o importam.
2. **(b)** Teste: uma construção C++ que hoje produz `Expr::Unsupported` sem
   tipo em posição de valor (escolha uma real, das que aparecem no corpus) e
   verifique que o Dart emitido carrega o tipo esperado do contexto, não
   `SyntaxBridgeOpaque`.
3. **(c)** Teste: uma construção C++ que hoje coloca um bailout numa posição de
   padrão, e verifique que o statement inteiro virou bailout de statement.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- Contagem de declarações de `SyntaxBridgeOpaque` no pacote: **1** (hoje 77).
  Verificação direta: `grep -rc "class SyntaxBridgeOpaque" .diagnosis/dart-package/lib/`.
- `invalid_override` → **zero**.
- `not_a_type`, `positional_field_in_object_pattern`,
  `refutable_pattern_in_irrefutable_context`,
  `pattern_type_mismatch_in_irrefutable_context`,
  `return_of_invalid_type_from_closure` → **zero**.
- `non_bool_condition` (46), `non_bool_negation_expression` (29),
  `non_bool_operand` (9), `constant_pattern_never_matches_value_type` (21),
  `undefined_setter` (15), `unnecessary_null_comparison` (18) → queda forte.
  (Parte de `non_bool_condition` é problema de parentização, tarefa 14.)
- `unchecked_use_of_nullable_value` (334) → queda forte.
- **Métrica de qualidade, não só de contagem:** o relatório em
  `.diagnosis/verovio-6.2.0.md` traz "Linhas stub (expressão)" e "Stub (%)".
  Esta tarefa **não** deve reduzir esses números de forma significativa: ela
  troca bailouts mal tipados por bailouts bem tipados, não os elimina. Se a
  contagem de stubs cair muito, você provavelmente apagou um bailout em vez de
  tipá-lo — isso é silêncio, e `AGENTS.md` proíbe.
- Nenhum `code` novo.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso possível em (c): quando o bailout sobe
de expressão para statement, ele pode engolir código que *era* traduzível ao
redor. Se para alguma construção real a escolha for entre "bailout maior mas
honesto" e "traduzir parcialmente e arriscar semântica errada", pergunte.

Dificuldade técnica não é motivo para parar.
