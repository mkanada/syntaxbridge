# Tarefa 08 — Templates de membro não são monomorfizados

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
família **T8**. Este prompt é autocontido.

## A causa raiz

O produto **já** monomorfiza templates: um template de **função livre**
instanciado tem seu nome derivado dos tipos concretos dos parâmetros por
`lower::cpp::monomorphized_template_name` (`crates/server/src/lower/cpp.rs:136`),
e tanto a declaração (`function_catalog.rs:3319-3338`) quanto o call site
(`lower_call_expr`) recomputam esse nome do mesmo jeito, "para que uma
especialização e seus call sites nunca acabem nomeando-a de formas diferentes".

O mesmo caminho **não** é percorrido para um método-template de um registro.
Quando o `libclang` resolve uma chamada `json.get<std::string>("x")`, o
`referenced` é a instanciação; `clang_getSpecializedCursorTemplate` devolve o
template primário — mas nada foi lowered para o registro, então não há
declaração nenhuma para o call site nomear.

Para **operadores** membro-template, o código já reconhece explicitamente a
lacuna e bailouta (`crates/server/src/lower/cpp.rs:10583-10608`, com um
comentário de 25 linhas descrevendo exatamente este problema). Para métodos
comuns, ele nem bailouta: emite uma chamada a um nome que não existe.

## A evidência

`dart analyze` (`.diagnosis/verovio-6.2.0.analyze.json`, commit `32dd1df`):

| `code` | n | forma |
| --- | ---: | --- |
| `undefined_method` | 133 | `The method 'get' isn't defined for the type 'JsonxxObject'` |
| `undefined_method` | 118 | `The method 'has' isn't defined for the type 'JsonxxObject'` |

Distribuição: `editortoolkit_neume.dart` 69, `editortoolkit_shared.dart` 25,
`toolkit.dart` 16, `iopae.dart` 15, `humlib.dart` 5, `docselection.dart` 3.

Mais **161 bailouts** `call to a member operator template instantiation — not
yet monomorphized`.

`.diagnosis/dart-package/lib/docselection.dart:38-39`:

```dart
if (json.has('measureRange')) {
  m_measureRange = json.get('measureRange');
```

O C++ (`src/docselection.cpp`) é:

```cpp
if (json.has<jsonxx::String>("measureRange")) {
    m_measureRange = json.get<jsonxx::String>("measureRange");
```

`jsonxx::Object::has<T>()` e `get<T>()` são templates de membro
(`src/json/jsonxx.h`). Nenhum dos dois foi emitido em `jsonxx.dart` — a classe
`JsonxxObject` sai com `parseStatic`, `json`, `xml`, `importObject`,
`importStringValue` e mais nada.

Isso é o caminho de entrada de **toda** a API pública do Verovio: o `toolkit`
recebe opções em JSON, e o editor de neumas conversa por JSON.

## O que fazer

1. **Reconhecer o método-template na travessia.**
   `function_catalog::function_declaration_kind_for` (por volta da linha 3813)
   mapeia `CXCursor_CXXMethod`/`CXCursor_ConversionFunction` para `Method` e
   `CXCursor_FunctionTemplate` para `FunctionTemplate`. Um método-template
   também chega como `CXCursor_FunctionTemplate`, mas com pai semântico sendo
   uma classe — hoje ele cai no ramo de função livre e não vira `ir::Method`
   de registro nenhum.

2. **Lowerar cada instanciação usada, uma vez por conjunto de argumentos de
   template.** É o mesmo modelo que já vale para função livre: não existe
   genérico no IR, existe uma cópia monomorfizada por instanciação. O nome sai
   de `monomorphized_template_name`, que hoje deriva o sufixo dos **parâmetros**
   — e `get<std::string>()` não tem parâmetro nenhum de tipo `T`. Estenda a
   função para incluir também os **argumentos de template** e o **tipo de
   retorno**, senão `get<String>` e `get<Number>` colidem no mesmo nome.

   `clang_Cursor_getNumTemplateArguments` / `clang_Cursor_getTemplateArgumentType`
   são as APIs para isso. **Confirme empiricamente** que elas respondem para o
   cursor de instanciação que você tem em mãos, e registre no comentário o que
   observou — a disciplina do repositório é essa.

3. **A declaração e o call site precisam concordar por construção.** Siga o
   padrão que o comentário de `function_catalog.rs:3319-3338` descreve: o call
   site recomputa o nome a partir do template primário + argumentos, em vez de
   consultar uma tabela. Se você precisar de uma tabela, ela tem de ser
   preenchida no mesmo lugar que emite a declaração.

4. **Chamadas com instanciação que não é usada em lugar nenhum não devem gerar
   declaração** — o `dart analyze` reclamaria de `unused_element`.

5. **Escopo.** Fica de fora, com bailout explícito e mensagem clara:
   - template de membro cujo argumento de template é outro template
     (`get<std::vector<int>>()`);
   - template de **classe** definido pelo projeto (não pela stdlib) —
     `jsonxx::Object` não é template de classe, então o corpus não força isso.

   Registre no resumo quantos bailouts sobraram por cada exclusão.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro**, no formato exato do jsonxx (estilo
   `crates/server/tests/lower_cpp.rs` + `emit_dart.rs`):

   ```cpp
   #include <string>
   class Caixa {
   public:
       template <typename T> bool tem(const std::string &chave) const;
       template <typename T> T pega(const std::string &chave) const;
   };
   template <> bool Caixa::tem<std::string>(const std::string &chave) const { return true; }
   template <> std::string Caixa::pega<std::string>(const std::string &chave) const { return chave; }

   std::string usa(const Caixa &c) {
       if (c.tem<std::string>("a")) return c.pega<std::string>("a");
       return "";
   }
   ```

   Verifique que `Caixa` no Dart emitido declara os dois métodos com nomes
   monomorfizados e que `usa` os chama pelos mesmos nomes.

2. **Teste de duas instanciações** do mesmo método (`pega<std::string>` e
   `pega<int>`), garantindo dois nomes distintos e nenhum
   `duplicate_definition`.

3. **`examples/E08-templates/`** já existe e cobre templates de função livre —
   ele é a rede de segurança contra regressão. Se o exemplo não cobrir método
   de classe, acrescente o caso lá, com `oracle/cases.json`.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `undefined_method` sobre `'get'`/`'has'` em `JsonxxObject`: **251 → 0**.
- Bailouts `call to a member operator template instantiation`: **161 → 0** ou
  perto disso.
- `undefined_method` total: queda de ~250.
- Verificação direta: `grep -n "has\|get" .diagnosis/dart-package/lib/jsonxx.dart`
  deve mostrar métodos declarados na classe `JsonxxObject`.
- Nenhum `code` novo; nenhuma das três contagens de bailout sobe.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível: o esquema de nome
monomorfizado é visível no Dart gerado (`pegaString`, `pegaInt`), e alguém pode
preferir genéricos Dart de verdade (`T pega<T>(String chave)`) quando o corpo
do template não depende do tipo. Isso é uma mudança de forma do produto e não
cabe nesta tarefa — mas se o corpus tiver muitos casos em que o genérico Dart
seria trivialmente correto, traga a contagem e pergunte.

Dificuldade técnica não é motivo para parar.
