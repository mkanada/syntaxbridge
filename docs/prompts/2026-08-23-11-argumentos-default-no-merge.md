# Tarefa 11 — Argumentos default vivem na declaração e somem na definição

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
família **T11**. Este prompt é autocontido.

Tarefa **pequena e precisa**. Duas metades independentes.

## Metade A — o default some quando a definição é out-of-line

### A causa raiz

O C++ exige que um argumento default apareça **uma única vez**, e por convenção
ele fica na **declaração** (o header). A definição repete a assinatura sem o
default:

```cpp
// include/hum/humlib.h:4047
static std::string durationToRecip(HumNum duration, HumNum scale = 1);

// src/hum/humlib.cpp:5208
string Convert::durationToRecip(HumNum duration, HumNum scale) { … }
```

`lower::cpp::collect_params_with_clone_prelude`
(`crates/server/src/lower/cpp.rs:2900-2940`) lê o default do cursor
`ParmVarDecl` que estiver na sua frente — e no cursor da *definição* não há
default nenhum.

`function_catalog::merge_methods` (`crates/server/src/function_catalog.rs:547`)
resolve o conflito entre partials preferindo a versão **com corpo**:

```rust
if existing[index].body.is_none() && method.body.is_some() {
    existing[index] = method;   // ← substitui o Method inteiro, params inclusive
}
```

Isso está certo para o corpo e errado para os parâmetros: a substituição é
**do registro inteiro**, e leva junto a lista de parâmetros sem defaults.

O jeito mais robusto de consertar **não** é mexer no merge: é ler o default do
lugar certo já no lowering. `clang_getCanonicalCursor` devolve a primeira
declaração de um símbolo — a do header, a que tem o default. Duas declarações do
mesmo símbolo têm o mesmo USR, então o mapeamento é direto.

### A evidência

`.diagnosis/dart-package/lib/humlib.dart:26169`:

```dart
static String durationToRecip(HumNum duration, HumNum scale) {
```

e 20 call sites com um argumento só (`humlib.dart:13595`,
`Convert.durationToRecip(dur)`), cada um produzindo

```
2 positional arguments expected by 'durationToRecip', but 1 found.
```

Contagem dos **144 `not_enough_positional_arguments`**, por função:

| n | função |
| ---: | --- |
| 49 | `stoi` (é a metade B) |
| 20 | `durationToRecip` |
| 19 | `recipToDurationStringHumNumString` |
| 12 | `assignFrom` |
| 10 | `getMeasureTstampNullableHumdrumTokenIntHumNum` |
| 9 | `recipToDurationNullableStringHumNumString` |
| 5 | `getMeasureTstampPlusDur` |
| 4 + 3 | `stof`, `stod` (metade B) |

## Metade B — `std::stoi`/`stod`/`stof` não estão na tabela de adaptadores

### A causa raiz

`BRIDGED_STDLIB_FREE_FUNCTION_NAMES` (`crates/server/src/lower/cpp.rs:9959`) é

```rust
&["gcd", "max", "min", "abs", "to_string", "make_pair", "swap"]
```

`std::stoi` não está lá. Ele cai então na detecção automática de fronteira
externa, que emite um mock com a assinatura completa do header
(`.diagnosis/dart-package/lib/basic_string.dart:3`):

```dart
int stoi(String __str, SyntaxBridgeOpaque /* unsupported: size_t * */ __idx, int __base) { … }
```

Três parâmetros obrigatórios, e todo call site real (`stoi(s)`) quebra.

### O que fazer

Acrescentar à tabela e a `lower_stdlib_free_function_call`
(`crates/server/src/lower/cpp.rs:9995`) os conversores de string, com tradução
direta:

| C++ | Dart |
| --- | --- |
| `std::stoi(s)` | `int.parse(s)` |
| `std::stol(s)`, `std::stoll(s)` | `int.parse(s)` |
| `std::stod(s)`, `std::stof(s)` | `double.parse(s)` |
| `std::stoi(s, nullptr, 16)` | `int.parse(s, radix: 16)` |

`std::stoi` com o parâmetro `size_t *idx` (posição final) **não** tem
equivalente direto — bailout explícito nesse caso, com a mensagem dizendo qual
sobrecarga. Verifique se o Verovio usa essa forma antes de gastar tempo com ela.

Enquanto estiver na tabela, verifique se estas outras funções livres, que hoje
aparecem como `undefined_method` no relatório, também merecem entrar (leia o
`docs/prompts/2026-08-21-07-adaptadores-de-stdlib-e-libc.md`, que estabeleceu
essa tabela, antes de mexer):

| n em `undefined_method` | função |
| ---: | --- |
| 170 | `vector` — na verdade `std::vector<T>(n, v)`, construção, não chamada |
| 116 | `fill` |
| 24 | `pair` |
| 11 | `make_tuple` |
| 6 | `reverse_copy` |

Cada uma que você **não** incluir tem de ficar registrada no resumo com a
contagem, para a próxima rodada saber o que sobrou.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** para a metade A, em **duas** unidades de
   compilação (o formato importa — veja `examples/E11-multi-tu/`):

   ```cpp
   // calc.h
   struct Calc { static int passo(int base, int fator = 2); };

   // calc.cpp
   #include "calc.h"
   int Calc::passo(int base, int fator) { return base * fator; }

   // uso.cpp
   #include "calc.h"
   int usa() { return Calc::passo(21); }
   ```

   Verifique que o Dart emitido declara `static int passo(int base, [int fator = 2])`
   e que `usa` chama `Calc.passo(21)` sem erro.

2. **Teste que falha primeiro** para a metade B:

   ```cpp
   #include <string>
   int paraInt(const std::string &s) { return std::stoi(s); }
   double paraDouble(const std::string &s) { return std::stod(s); }
   ```

   Verifique `int.parse(s)` / `double.parse(s)` no Dart emitido e nenhum mock
   `stoi` sobrando.

3. **`examples/E07-sobrecarga-e-parametros-default/`** já cobre defaults numa
   unidade só e tem oráculo — não pode regredir.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `not_enough_positional_arguments`: **144 → abaixo de 20**.
- `grep -n "stoi" .diagnosis/dart-package/lib/basic_string.dart` → o mock some
  (e provavelmente o arquivo inteiro, se `stoi` for a única coisa nele).
- Nenhum `code` novo; nenhuma das três contagens de bailout sobe.

## Quando parar e perguntar

Não deve haver decisão de produto nesta tarefa. Se você achar uma — por
exemplo, um default cuja expressão não é constante em Dart (`= Foo()`), que a
tarefa 15.4 do lote anterior já tratou para um caso — resolva pelo padrão já
estabelecido nela e registre.

Dificuldade técnica não é motivo para parar.
