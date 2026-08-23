# Tarefa 05 — `const char*` é `String?`, `std::string` é `String`, e falta a ponte

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
família **T5**. Este prompt é autocontido.

## A causa raiz

Duas decisões de mapeamento, ambas corretas isoladamente, que não se falam:

- `const char*` → `Type::Nullable(Type::Str)` → Dart `String?`. A justificativa
  está no comentário de `lower_type` (`crates/server/src/lower/cpp.rs:1905-1918`):
  um ponteiro é nulo ou uma cadeia de bytes real, e essa é a mesma garantia
  finita-e-anulável de qualquer outro ponteiro.
- `std::string` → `Type::Str` → Dart `String`, nunca anulável.

Em C++, a passagem de um para o outro é uma **conversão implícita**: o
construtor convertente de `std::string` a partir de `const char*`. É ali que o
contrato "não é nulo" é assegurado (`std::string(nullptr)` é comportamento
indefinido; todo código que faz isso está assumindo não-nulo).

`lower::cpp` trata esse construtor como **passagem transparente**
(`crates/server/src/lower/cpp.rs:7233-7237`):

```rust
if arg_count >= 1 && owner_template_name.as_deref() == Some("basic_string") {
    let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    return unsafe { lower_expr(arg_cursor, project_root) };
}
```

O argumento passa adiante **com o tipo dele** — `String?` — para uma posição que
espera `String`. A conversão que dava a garantia foi apagada.

## A evidência

`dart analyze` (`.diagnosis/verovio-6.2.0.analyze.json`, commit `32dd1df`):

| `code` | n | forma |
| --- | ---: | --- |
| `argument_type_not_assignable` | 1.043 | `'String?'` → parâmetro `'String'` |
| `invalid_assignment` | 177 | `'String?'` → variável `'String'` |

Concentração por arquivo: `atts_shared.dart` 264, `humlib.dart` 259,
`iohumdrum.dart` 171, `atts_visual.dart` 85, `iomusxml.dart` 47,
`atts_cmn.dart` 46, `iomei.dart` 38.

`.diagnosis/dart-package/lib/atts_analytical.dart:29`:

```dart
SetForm(StrToHarmAnlForm(element.attributeNullableStringConst('form').value()));
```

O C++ (`libmei/dist/atts_analytical.cpp:44`) é:

```cpp
this->SetForm(StrToHarmAnlForm(element.attribute("form").value()));
```

`pugi::xml_attribute::value()` devolve `const char*` (e **nunca** nulo — o
pugixml devolve `""`); `StrToHarmAnlForm` recebe `std::string`. A conversão
implícita está exatamente entre os dois.

Repare que este é o mesmo arquivo que a família T7 (conversão *safe bool* do
pugixml) também atinge. As duas famílias juntas explicam por que todas as
centenas de funções `Read*`/`Write*` de `libmei/dist/` estão quebradas.

## O que fazer

**Materializar a conversão em vez de apagá-la.** Onde o C++ constrói uma
`std::string` a partir de um `const char*`, o Dart precisa de um valor
não-anulável:

1. No braço de `basic_string` de `lower_constructor_call`
   (`crates/server/src/lower/cpp.rs:7233`), não devolver o argumento cru.
   Devolver o argumento **com a garantia materializada** quando o tipo do
   argumento for `Nullable(Str)`:

   - Se o argumento for um literal de string, ele já é `Type::Str` — nada muda.
   - Caso contrário, envolva-o num nó que preserve o tipo estático `Type::Str`.
     Duas formas possíveis; escolha **uma** e use em todo lugar:

     | Forma | Dart | Semântica |
     | --- | --- | --- |
     | asserção | `x!` | falha alto se for nulo — espelha o UB do C++ tornando-o visível |
     | valor neutro | `x ?? ''` | nunca falha — espelha o que o pugixml de fato devolve |

     A recomendação é a **asserção**: `AGENTS.md` exige falhar explicitamente em
     vez de silenciar, e `x ?? ''` transforma um bug real do original em uma
     string vazia silenciosa. Mas isto é decisão de produto — veja "Quando
     parar e perguntar".

   Se `ir::Expr` ainda não tiver um nó de asserção de não-nulidade, o caminho
   mais barato é reaproveitar `Expr::Convert` (que já existe e já é o nó de
   "conversão na fronteira") com `Type::Str` como destino, e ensinar
   `emit::dart` a imprimir `!` para o par `Nullable(Str)` → `Str`.

2. **A mesma conversão acontece em mais três posições**, e todas passam pelo
   mesmo lugar no C++ (`ImplicitCastExpr` / `CXXConstructExpr` de
   `basic_string`): argumento de chamada, inicialização de variável, e `return`.
   Verifique se as três chegam ao braço que você corrigiu; se alguma delas
   estiver sendo lowered por outro caminho, corrija lá também.

3. **Não mexa no mapeamento `const char*` → `String?`.** Ele está certo: um
   `const char*` pode ser nulo, e o Dart deve continuar dizendo isso. O que
   muda é só o ponto de travessia.

4. **Cuidado com a direção oposta**, que também aparece: `std::string` →
   `const char*` (via `c_str()`). O braço `("basic_string", "c_str")`
   (`crates/server/src/lower/cpp.rs:8562`) já devolve o alvo direto, e ali
   `String` → `String?` é um alargamento seguro. Nada a fazer, mas confirme
   que continua assim depois da mudança.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** (estilo `crates/server/tests/lower_cpp.rs` +
   `emit_dart.rs`):

   ```cpp
   #include <string>
   struct Fonte {
       const char *bruto() const { return "x"; }
   };
   std::string normaliza(const std::string &s) { return s; }
   std::string usa(const Fonte &f) { return normaliza(f.bruto()); }
   ```

   Verifique que o Dart emitido para `usa` não passa um `String?` para
   `normaliza(String)` e que `dart analyze` sobre o pacote não reporta erro.

2. **Teste de inicialização e de retorno**:

   ```cpp
   std::string direto(const Fonte &f) {
       std::string s = f.bruto();
       return f.bruto();
   }
   ```

3. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`. `examples/E05-biblioteca-padrao/` e
   `examples/E13-fatia-real-verovio/` são a rede de segurança.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `argument_type_not_assignable` com `'String?'` → `'String'`: **1.043 → 0**.
- `invalid_assignment` com `'String?'` → `'String'`: **177 → 0**.
- `argument_type_not_assignable` total: cai ~1.043 em relação ao valor da
  rodada anterior a esta tarefa.
- Nenhum `code` novo; nenhuma das três contagens de bailout sobe.

## Quando parar e perguntar

**Pergunte antes de implementar, uma vez.** A escolha entre `x!` (falha alto) e
`x ?? ''` (valor neutro) é decisão de produto: ela muda o comportamento do
programa gerado quando o `const char*` original *é* nulo. Apresente as duas com
a recomendação (`x!`, por ser a que não esconde o bug) e siga com a resposta.
Se não houver resposta, siga com `x!` e registre a suposição no resumo — é o
comportamento que o `AGENTS.md` favorece.

Dificuldade técnica não é motivo para parar.
