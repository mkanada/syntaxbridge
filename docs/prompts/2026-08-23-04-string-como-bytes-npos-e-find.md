# Tarefa 04 — `std::string` como bytes: `npos`, `find` e `char`

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
família **T4**. Este prompt é autocontido.

## A causa raiz

O bridge representa `std::string` como `String` do Dart, mas indexa **em
bytes** — porque em C++ `s[i]` é um `char`, e `String` do Dart é indexada em
UTF-16. Para preservar a semântica, existem três nós de IR
(`crates/server/src/ir/mod.rs`, por volta da linha 470):

| IR | emitido como (`crates/server/src/emit/dart.rs:3895-3917`) |
| --- | --- |
| `Expr::StringByteLength` | `utf8.encode(s).length` |
| `Expr::StringByteIndexOf` | `utf8.encode(a).indexOf(utf8.encode(b))` |
| `Expr::StringByteAt` | `utf8.encode(s)[i]` |

A escolha de indexar em bytes é defensável e **não** é o que esta tarefa
questiona. O que está errado é a implementação, em três pontos:

1. **`utf8.encode(...)` devolve `Uint8List`, e `Uint8List.indexOf` recebe um
   `int` (um elemento), não uma lista.** Então `StringByteIndexOf` é sempre um
   erro de tipo quando a agulha tem mais de um byte.
2. **Quando a agulha é um `char`**, o bridge já a mapeou para `int`, e
   `utf8.encode(34)` não compila (`utf8.encode` recebe `String`).
3. **`std::string::npos` não tem mapeamento nenhum** e sai como
   `basic_string.npos` — um identificador que não existe. Ele chega aqui como
   um `CXCursor_VarDecl` estático cujo dono é `basic_string`, e
   `lower::cpp::qualified_static_member_name` (`crates/server/src/lower/cpp.rs:1652`)
   o qualifica pelo nome do dono, como faria com qualquer constante de classe.

Há também um quarto ponto, do mesmo caminho: `std::string += char` sai como
`String + int` (`humlib.dart:1172` → `output = output + 45;`). O braço
`("basic_string", "operator+=")` de `lower_stdlib_method_call`
(`crates/server/src/lower/cpp.rs:4941`) constrói um `Binary::Add` sem olhar se
o operando é um `char`.

## A evidência

`dart analyze` (`.diagnosis/verovio-6.2.0.analyze.json`, commit `32dd1df`)
atribui a esta família ~2.370 diagnósticos:

| `code` | n | forma |
| --- | ---: | --- |
| `undefined_identifier` | 798 | `Undefined name 'basic_string'` (é sempre `basic_string.npos`) |
| `argument_type_not_assignable` | 769 | `'Uint8List'` → parâmetro `'int'` |
| `argument_type_not_assignable` | 666 | `'int'` → parâmetro `'String'` |
| `undefined_identifier` | 116 | `Undefined name 'length'` (`midimessage.dart`, `humlib.dart`) |
| `invalid_assignment` | 23 | `'int'` → variável `'String'` |

Concentração: `humlib.dart` (1.200+), `iohumdrum.dart` (260+), `pugixml.dart`
(126), `docselection.dart`, `binasc.dart`, `ioabc.dart`.

`.diagnosis/dart-package/lib/docselection.dart:44-47` mostra os três problemas
numa tela só:

```dart
if (utf8.encode(m_measureRange).indexOf(utf8.encode('-')) != basic_string.npos) {
  int pos = utf8.encode(m_measureRange).indexOf(utf8.encode('-'));
  String startRange = m_measureRange.substring(0, 0 + pos);
  String endRange = m_measureRange.substring(pos + 1, pos + 1 + basic_string.npos);
```

O C++ (`src/docselection.cpp`) é o idioma mais comum que existe:

```cpp
if (m_measureRange.find("-") != std::string::npos) {
    int pos = m_measureRange.find("-");
    std::string startRange = m_measureRange.substr(0, pos);
    std::string endRange = m_measureRange.substr(pos + 1);
```

E `.diagnosis/dart-package/lib/binasc.dart:159`:

```dart
if (utf8.encode(terminators).indexOf(utf8.encode(34)) != basic_string.npos) {
```

`34` é `'"'` — um `char` literal, que o bridge mapeia para `int`.

## O que fazer

O ponto central: **a busca por byte precisa de um helper de verdade em
`syntax_bridge_support.dart`**, não de uma composição de métodos do
`dart:convert` que por acaso não encaixa.

1. **`npos` → `-1`.** Em C++, `find` devolve `std::string::npos` quando não
   acha; em Dart, `indexOf` devolve `-1`. As comparações `!= npos` / `== npos`
   traduzem-se diretamente. Reconheça o `VarDecl` estático `npos` cujo dono é
   `basic_string` (`lower_type` já sabe identificar o template `basic_string`
   — veja `stdlib_template_name`, `crates/server/src/lower/cpp.rs:2509`) e
   emita o literal `-1`.

   **Cuidado com um uso legítimo diferente:** `substr(pos)` sem segundo
   argumento e `substr(pos, npos)` significam "até o fim". Se `npos` virar
   `-1` cegamente, `substring(a, a + (-1))` fica errado. O braço
   `("basic_string", "substr")` (`cpp.rs:8636`) já trata o caso de um
   argumento; o caso de dois argumentos com `npos` no segundo precisa cair no
   mesmo lugar.

2. **Um helper de busca por bytes.** Em `syntax_bridge_support.dart`, algo como:

   ```dart
   int syntaxBridgeIndexOfBytes(String haystack, String needle, [int from = 0]) { … }
   int syntaxBridgeIndexOfByte(String haystack, int byte, [int from = 0]) { … }
   ```

   `StringByteIndexOf` passa a emitir uma chamada a um dos dois, escolhida pelo
   tipo estático da agulha (`Type::Str` → o primeiro; `Type::Int` → o segundo).
   Os dois devolvem `-1` quando não acham, o que fecha o par com o item 1.

   `ir::Expr::StringByteIndexOf` hoje não carrega o tipo da agulha; ele carrega
   a `Expr`, e a `Expr` tem tipo. Use `expr_ty` do emissor, o mesmo caminho que
   `receiver_bang` já usa.

3. **`std::string += char` e `std::string + char`.** Quando o operando é um
   `char` (que o bridge mapeia para `Type::Int`), a concatenação precisa de
   `String.fromCharCode(...)`. O emissor já produz essa forma em alguns lugares
   (`binasc.dart:171` →
   `word = word + String.fromCharCode(utf8.encode(input)[i]);`), então a peça
   existe: falta aplicá-la no braço `operator+=`/`operator+` de `basic_string`.

4. **`Undefined name 'length'` (116).** Investigue: a concentração em
   `midimessage.dart` (94) sugere um membro `length` de uma classe do domínio
   sendo lido sem receptor. Confirme com o C++ correspondente
   (`src/midi/MidiMessage.cpp`) antes de mexer — pode ser outra família (T2 ou
   T7) e não pertencer a esta tarefa. Se for, registre e deixe fora.

5. **Reveja `basic_string::find` com dois argumentos.** Hoje o braço
   (`cpp.rs:8563`) rejeita qualquer coisa diferente de exatamente 1 argumento.
   `find(needle, from)` é comum; o helper do item 2 já tem o parâmetro `from`.
   Aproveite. O mesmo para `rfind`, se aparecer no corpus.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** (estilo `crates/server/tests/lower_cpp.rs` +
   `emit_dart.rs`), o idioma completo do `docselection.cpp`:

   ```cpp
   #include <string>
   std::string antes(const std::string &s) {
       if (s.find("-") != std::string::npos) {
           return s.substr(0, s.find("-"));
       }
       return s;
   }
   ```

   Verifique que o Dart emitido não contém `basic_string` nem
   `utf8.encode(...).indexOf(utf8.encode(...))`, e que `dart analyze` sobre o
   pacote gerado não reporta erro.

2. **Teste do `char`**:

   ```cpp
   #include <string>
   bool temAspas(const std::string &s) { return s.find('"') != std::string::npos; }
   std::string comQuebra(std::string s) { s += '\n'; return s; }
   ```

3. **Teste comportamental**: `examples/E05-biblioteca-padrao/` já cobre
   `std::string`; acrescente casos de `find`/`npos`/`substr` ao seu
   `oracle/cases.json` para que o resultado seja **executado**, não só
   analisado. Esta é a única prova real de que a semântica de byte foi
   preservada.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `grep -rc "basic_string" .diagnosis/dart-package/lib/` → **zero**.
- `undefined_identifier`: **1.245 → abaixo de 350**.
- `argument_type_not_assignable`: **3.001 → abaixo de 1.600** (os 769
  `Uint8List`→`int` e os 666 `int`→`String` somem).
- Nenhum `code` novo; nenhuma das três contagens de bailout sobe.
- `examples/E05` continua passando, incluindo o oráculo.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível: manter a indexação em bytes
custa uma conversão a cada operação, e uma alternativa seria representar
`std::string` como `List<int>`/`Uint8List` de ponta a ponta, com conversão só
nas fronteiras de I/O. Isso muda o tipo de dezenas de assinaturas públicas do
pacote gerado e é decisão de produto — **não a tome sozinho**. Se, ao medir,
o custo da forma atual parecer proibitivo, traga números e pergunte.

Dificuldade técnica não é motivo para parar.
