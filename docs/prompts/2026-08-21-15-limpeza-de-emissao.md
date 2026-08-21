# Tarefa 15 — Limpeza de emissão: nove resíduos independentes

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido). Veja também
`docs/plans/estilo-de-codigo-gerado.md`.

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F15**. Este prompt é autocontido.

**Execute esta tarefa por último.** Vários dos itens abaixo mudam de tamanho
depois das tarefas 01-14; re-meça antes de começar cada um.

## O que esta tarefa é

Nove correções **independentes** de emissão, agrupadas num prompt só porque cada
uma é local e pequena. Faça-as como incrementos separados, cada um com seu
teste que falha primeiro, e não misture os commits.

Todos os números vêm de `.diagnosis/verovio-6.2.0.analyze.json` (24.791
diagnósticos sobre o pacote emitido do Verovio 6.2.0) e os exemplos de
`.diagnosis/dart-package/lib/`.

---

### 15.1 — `break` depois de `return` num `case` (`dead_code`, 83)

`.diagnosis/dart-package/lib/accid.dart:238-248`:

```dart
static int GetAccidGlyph(data_ACCIDENTAL_WRITTEN accid) {
  switch (accid.value) {
    case 1:
      return 57954;
      break;        // ← Dead code.
    case 2:
      return 57952;
      break;        // ← Dead code.
```

Em C++ o `break` é obrigatório em muitos estilos e inofensivo depois do
`return`; em Dart é código morto. Suprimir o terminador quando o corpo do
`case` já termina (`return`, `throw`, `continue`).

Concentração: `accid.dart` (32), `iohumdrum.dart` (10), `attalternates.dart`
(9), `editortoolkit_neume.dart` (9). Nota: `vrv.dart:282` → `if (false) {` é
`dead_code` por outro motivo (uma condição constante do C++ original) e pode
ser legítimo — verifique antes de suprimir.

**Onde:** `crates/server/src/emit/dart.rs`, emissão de `Stmt::Switch`.
**Alvo:** `dead_code` → perto de zero.

---

### 15.2 — `import` não usado (`unused_import`, 143)

`.diagnosis/dart-package/lib/calcalignmentpitchposfunctor.dart:13` →
`import 'nc.dart';`, sem nenhuma referência a `Nc` no arquivo.

Mais frequentes: `att.dart` (19), `attconverter.dart` (19),
`boundingbox.dart` (8), `object.dart` (7). Em 50 arquivos.

**Onde:** `crates/server/src/emit/dart.rs`, o cálculo de dependências por
arquivo (por volta de 530-560, onde os `usr` referenciados por um registro são
reunidos). O conjunto está largo demais — provavelmente inclui dependências
transitivas de mixin que o `with` já não menciona.

**Cuidado:** as tarefas 01, 06 e 07 *adicionam* referências (métodos que
voltam, o arquivo de suporte, `dart:math`). Meça depois delas.

**Alvo:** `unused_import` → zero, sem que `undefined_class` /
`undefined_identifier` subam (o erro oposto: remover um import necessário).

---

### 15.3 — `SyntaxBridgePair.first`/`second` são `final` (`assignment_to_final`, 39)

`.diagnosis/dart-package/lib/iohumdrum.dart:9192` → `v.second = i;`

`std::pair` em C++ é mutável: `p.first = x` é legal e comum. O adaptador em
`.diagnosis/dart-package/lib/syntax_bridge_support.dart` declara os dois campos
como `final`:

```dart
final class SyntaxBridgePair<A, B> {
  const SyntaxBridgePair(this.first, this.second);
  final A first;
  final B second;
}
```

Tornar os campos mutáveis. Isso implica perder o construtor `const` — verifique
se algum ponto do pipeline depende dele.

Concentração: `iohumdrum.dart` (26), `iocmme.dart` (7), `ioabc.dart` (3).
**Onde:** `crates/server/src/emit/dart.rs`, o texto do arquivo de suporte.
**Alvo:** `assignment_to_final` → zero.

---

### 15.4 — Default de parâmetro não-constante (15)

Dart exige que o valor default de um parâmetro seja uma constante de
compilação.

```dart
// editortoolkit_neume.dart:2695 — const_eval_method_invocation (10)
int distanceToBB(int ulx, …, [double rotate = 0.toDouble()]) {

// devicecontext.dart:146 — const_eval_property_access (5)
void SetBackground(int color, [int style = PenStyle.PEN_SOLID.value]);
```

`0.toDouble()` deve ser emitido como o literal `0.0`. `PenStyle.PEN_SOLID.value`
precisa da constante inteira correspondente, ou o parâmetro precisa ser tipado
pelo enum em vez de `int`.

**Onde:** `crates/server/src/lower/cpp.rs`, o lowering de
`ir::Param::default_value` (o doc comment de `Param` explica que default de
C++ mapeia 1:1 para parâmetro opcional Dart); e
`crates/server/src/emit/dart.rs`, a renderização do default.
**Alvo:** `const_eval_method_invocation` e `const_eval_property_access` → zero.

---

### 15.5 — `for` sobre `String` e `Map` (`for_in_of_invalid_type`, ~16 dos 24)

```dart
// bboxdevicecontext.dart:343
for (int c in text) {                 // text é String
// adjustaccidxfunctor.dart:31
for (… in mapa) {                     // Map<int, GraceAligner?>
```

Em C++, `for (auto c : str)` percorre os caracteres e `for (auto &kv : mapa)`
percorre os pares. Em Dart, `String` e `Map` não são `Iterable`.

- `String` → `.codeUnits` (se o corpo trata `c` como inteiro) ou `.split('')`
  (se trata como caractere). O tipo declarado da variável do laço diz qual.
- `Map` → `.entries`, com `kv.first`/`kv.second` do C++ virando
  `kv.key`/`kv.value`.

Os 8 restantes (`xpath_node_set`) são da tarefa 13.
**Onde:** `crates/server/src/lower/cpp.rs`, o lowering de `CXXForRangeStmt`.
**Alvo:** `for_in_of_invalid_type` → zero (assumindo a tarefa 13 feita).

---

### 15.6 — Chave de mapa com enum onde o tipo é `int` (`map_key_type_not_assignable`, 6)

`.diagnosis/dart-package/lib/alignfunctor.dart:65`:

```dart
Map<int, data_DURATION> durationEq = <int, data_DURATION>{
  option_DURATION_EQ.DURATION_EQ_brevis: data_DURATION.DURATION_brevis, … };
```

Em C++ o enum não-escopado converte implicitamente para `int`. Ou usar `.value`
na chave, ou tipar o mapa pelo enum. A segunda é mais fiel ao domínio e alinhada
com o `AGENTS.md` (mapeamento preciso, não erasure).

Ocorre em `alignfunctor.dart` (3) e `midifunctor.dart` (3).
**Alvo:** `map_key_type_not_assignable` → zero.

---

### 15.7 — Funções variádicas C++ (parte de `extra_positional_arguments`)

`.diagnosis/dart-package/lib/iocmme.dart:153` → `LogError('%s', str);` para uma
`LogError` que aceita 1 parâmetro.

`void LogError(const char *fmt, ...)` não tem equivalente posicional em Dart.
Segundo `AGENTS.md`, a resposta é uma fronteira nomeada e explícita — por
exemplo, uma assinatura `LogError(String fmt, [List<Object?> args = const []])`
com a formatação `printf` feita por um helper no arquivo de suporte — e nunca
uma chamada que passa mais argumentos do que a declaração aceita.

**Cuidado:** a maior parte dos 481 `extra_positional_arguments` é da tarefa 02
(construtores de registros que viraram mixin). Meça depois dela: o resíduo
variádico é da ordem de 100.

**Onde:** `crates/server/src/lower/cpp.rs`, o lowering de parâmetros de função
(`clang_Cursor_isVariadic`).
**Alvo:** o resíduo de `extra_positional_arguments` → zero.

---

### 15.8 — Local que esconde o nome do próprio tipo (`referenced_before_declaration`, 3)

`.diagnosis/dart-package/lib/zip_file.dart:1390`:

```dart
tm tm = tm(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, null);
```

C++ permite `struct tm tm;`; Dart não — o nome da variável esconde o do tipo, e
a inicialização passa a referenciar a variável em construção. Renomear a local
no lowering quando ela colidir com o nome do próprio tipo.

Ocorre em `zip_file.dart` (2) e `adjustgracexposfunctor.dart` (1).
**Alvo:** `referenced_before_declaration` → zero.

---

### 15.9 — `main` com assinatura C (`main_first_positional_parameter_type`, 1)

`.diagnosis/dart-package/lib/main.dart:43`:

```dart
int main(int argc, SyntaxBridgeOpaque /* unsupported: char ** */ argv) {
```

Dart exige `void main()` ou `void main(List<String> args)`. Emitir a assinatura
Dart e ligar `argc`/`argv` a `args.length`/`args` no prólogo do corpo.

**Alvo:** `main_first_positional_parameter_type` → zero.

---

## Método

TDD, conforme `AGENTS.md`, **um incremento por item**:

1. Teste que falha primeiro, com o fixture C++ mínimo que reproduz o item.
   `crates/server/tests/lower_cpp.rs` mostra o padrão.
2. Implemente até passar.
3. Só então passe ao item seguinte.
4. Ao fim de todos: `just test` (ou `just test-host`, registrando no resumo),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak), estes `code`s devem estar em **zero**:

`dead_code` (exceto ocorrências legítimas, documentadas uma a uma no resumo),
`unused_import`, `assignment_to_final`, `const_eval_method_invocation`,
`const_eval_property_access`, `for_in_of_invalid_type`,
`map_key_type_not_assignable`, `referenced_before_declaration`,
`main_first_positional_parameter_type`.

Mais o resíduo variádico de `extra_positional_arguments`.

Nenhum `code` novo, e a contagem total de erros e avisos não pode subir.

## Quando parar e perguntar

Só por decisão de **produto**. Dois candidatos:

- **15.3** — tornar `SyntaxBridgePair` mutável perde o construtor `const`. Se
  algum ponto do pipeline usa `SyntaxBridgePair` como valor constante, a
  alternativa é um par mutável separado, e aí passam a existir dois tipos de
  par no Dart gerado. Pergunte se isso aparecer.
- **15.7** — a forma da fronteira variádica (lista de `Object?` + formatação
  `printf` no arquivo de suporte, versus declarar variádicas como não
  transpiláveis e emitir bailout) muda o produto de forma observável, e
  `LogError`/`LogWarning`/`LogDebug` do Verovio são só a ponta: código C real
  usa `printf` o tempo todo. Pergunte antes de fixar.

Dificuldade técnica não é motivo para parar.
