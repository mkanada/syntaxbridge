# Tarefa 14 — Parentizar toda expressão composta na emissão

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido, silêncio proibido: uma
tradução plausível e errada é pior que um bailout explícito).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F14**. Este prompt é autocontido.

## A causa raiz

O emissor imprime expressões compostas sem parênteses, confiando em que a
precedência do Dart coincida com a do C++ — ou, pior, confiando em que a forma
Dart gerada (que às vezes não corresponde a nenhuma expressão C++, como um
ternário criado para representar uma conversão `int`→`bool`) tenha a
precedência esperada no contexto onde é colada.

Essa família é pequena em número de diagnósticos, mas **cada ocorrência é
código silenciosamente errado**: o Dart compila e faz outra coisa. É o tipo de
erro que o `dart analyze` só pega por acidente, quando a expressão mal
parentizada calha de ter um tipo incompatível.

## A evidência

### Caso 1 — ternário sintetizado, sem parênteses, numa condição

`.diagnosis/dart-package/lib/iohumdrum.dart:9035`:

```dart
if (solo ? 1 : 0 == true.toInt()) {
```

O C++ é `if (solo == true)` com `solo` de tipo inteiro. A conversão
`int`→`bool` virou um ternário e foi colada sem parênteses. Pela precedência do
Dart, isso é `solo ? 1 : (0 == true.toInt())` — o resultado é um `int`, e o
`dart analyze` só percebe porque `int` não é `bool`
(`non_bool_condition`). Se os tipos tivessem casado, a expressão estaria
silenciosamente errada.

### Caso 2 — ternário sem parênteses antes de `!` e `.`

`.diagnosis/dart-package/lib/editortoolkit_neume.dart:1835`:

```dart
oldClef = sparent is Layer ? sparent : null!.GetCurrentClef();
```

Deveria ser:

```dart
oldClef = (sparent is Layer ? sparent : null)!.GetCurrentClef();
```

Como está, o Dart lê `sparent is Layer ? sparent : (null!.GetCurrentClef())` —
uma chamada de método sobre o literal `null`, que sempre estoura. Daí os dois
avisos na mesma linha: `null_check_always_fails` e `receiver_of_type_never`.

Esta forma vem do lowering de `dynamic_cast` (`x is T ? x : null`,
`lower::cpp::lower_dynamic_cast_expr`, `crates/server/src/lower/cpp.rs` por
volta de 5049) sendo consumida como receptor de outra expressão.

### Caso 3 — `++`/`--` prefixo e postfix

`.diagnosis/dart-package/lib/pugixml.dart:3926`:

```dart
swapxpath_nodexpath_node((begin++)!, --end!);
//                                     ^^^^^ missing_assignable_selector
```

`--end!` é sintaticamente inválido em Dart. Deveria ser `--(end!)` ou, mais
provavelmente, uma forma diferente.

### Números

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791
diagnósticos) atribui a esta família ~**55** ocorrências:

| `code` | n | caso |
| --- | ---: | --- |
| `non_bool_condition` | até 46 | 1 (parte também é bailout, tarefa 06) |
| `null_check_always_fails` | 5 | 2 |
| `receiver_of_type_never` | 5 | 2 |
| `missing_assignable_selector` | 1 | 3 |

A contagem é o piso, não o teto: os casos em que a precedência errada **não**
causa incompatibilidade de tipo não aparecem em lugar nenhum do relatório.

## Onde mexer

- `crates/server/src/emit/dart.rs` — a renderização de `ir::Expr`. Os pontos
  a cobrir são todos os contextos em que uma subexpressão é colada dentro de
  outra: operando de binário/unário, receptor de `FieldAccess`/`Index`/`Call`,
  condição de `if`/`while`/ternário, argumento, lado direito de atribuição,
  operando de `!`.

A regra a adotar: **toda expressão composta emitida numa posição que não seja
um statement isolado é parentizada.** Não tente reproduzir a tabela de
precedência do Dart — parênteses redundantes são baratos (e o `dart format`, que
`transpile::transpile` já roda, remove muitos deles), enquanto um parêntese
faltando é um bug silencioso. "Composta" quer dizer: binário, unário, ternário,
`is`/`as`, atribuição usada como expressão. Um literal, uma referência simples,
uma chamada e um acesso a campo não precisam.

Um cuidado: `emit::dart::tuple_assign_needs_temp_block` documenta que a
gramática de *padrões* do Dart rejeita `!` dentro de um elemento de padrão. Há
outros contextos onde parênteses também são proibidos (não só desnecessários) —
posição de padrão é o principal. A regra precisa de exceção lá.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro, caso 1: um `if (i == true)` com `i` inteiro em
   C++. Verifique que o Dart emitido é uma condição `bool` correta.
2. Teste que falha, caso 2: um `dynamic_cast<T*>(x)->metodo()` em C++.
   Verifique que o downcast está parentizado antes do `!`/`.`.
3. Teste que falha, caso 3: `--p` sobre um ponteiro.
4. Um teste de amplitude: uma expressão aritmética aninhada
   (`a * (b + c) - d / (e - f)`) que produza o mesmo **valor** em C++ e no Dart
   emitido. Este é o teste que pega o caso silencioso.
5. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `null_check_always_fails` → **zero**.
- `receiver_of_type_never` → **zero**.
- `missing_assignable_selector` → **zero**.
- `non_bool_condition` → queda; o resíduo esperado são bailouts em posição de
  condição (tarefa 06). Se a tarefa 06 já tiver rodado, o alvo é zero.
- Nenhum `code` novo. Risco específico: parênteses em posição de padrão são
  erro de sintaxe — se `positional_field_in_object_pattern` ou
  `pattern_type_mismatch_in_irrefutable_context` subirem, a exceção de padrão
  não foi aplicada.
- **Arquivos que não parseiam como Dart**: a linha
  "Arquivos que não parseiam" em `.diagnosis/verovio-6.2.0.md` está em 1/301.
  Ela não pode subir.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso possível: parentizar tudo deixa o Dart
gerado mais ruidoso, e `docs/plans/estilo-de-codigo-gerado.md` trata legibilidade
do código emitido como requisito. Se o `dart format` (que
`transpile::transpile` já roda no caminho normal, e que o teste de diagnóstico
deliberadamente **não** roda) não limpar os redundantes o suficiente, a escolha
entre "parentizar tudo" e "parentizar por tabela de precedência" vira uma
troca real entre segurança e legibilidade — pergunte. Recomendação: parentizar
tudo; correção vale mais que estética em código gerado.

Dificuldade técnica não é motivo para parar.
