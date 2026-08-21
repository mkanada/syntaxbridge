# Tarefa 12 — Overloads que colidem no mesmo nome Dart

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F13**. Este prompt é autocontido.

**Execute a tarefa 01 antes desta.** Ela traz de volta os membros definidos
out-of-line, o que provavelmente **aumenta** o número de colisões antes de esta
tarefa reduzi-lo. Meça a linha de base depois da 01.

## A causa raiz

Dart não tem sobrecarga: dois membros com o mesmo nome no mesmo escopo são um
erro. O pipeline já sabe disso — `function_catalog::apply_overload_renames`
(`crates/server/src/function_catalog.rs`, por volta da linha 469) renomeia
overloads consultando `mapping::overload_options_for`, e o resultado aparece no
corpus: `Doc.GetOptions()` e `Doc.GetOptionsConst()` são o mesmo `GetOptions`
C++ em versão não-`const` e `const`.

Mas a desambiguação não cobre tudo. Duas lacunas:

**(a) `const`-ness nem sempre entra na chave.** `int GetX()` e
`int GetX() const` são dois membros distintos em C++ e o mesmo nome em Dart.
Como o par `GetOptions`/`GetOptionsConst` mostra que o mecanismo *existe*, isto
é inconsistência de aplicação, não ausência total — descubra por que ele
dispara em uns casos e não em outros.

**(b) Nomes-ponte de operador não são desambiguados.**
`lower::cpp::dart_operator_bridge_name` (`crates/server/src/lower/cpp.rs`, por
volta de 6285) mapeia `operator<<` → `streamInsert`, sem nenhum sufixo. Todos
os `operator<<` de um mesmo arquivo colidem entre si.

## A evidência

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791
diagnósticos):

`duplicate_definition` — **118** ocorrências, 17 arquivos: `humlib.dart` (41),
`jsonxx.dart` (29), `pugixml.dart` (18), `iohumdrum.dart` (9),
`boundingbox.dart` (4), `zip_file.dart` (4)…

Nomes que colidem:

| nome | n | causa |
| --- | ---: | --- |
| `streamInsert` | 36 | **(b)** — todos os `operator<<` de `humlib.dart` |
| `get` | 11 | (a) |
| `importInt` | 7 | (a) |
| `is_` | 6 | (a) |
| `write` | 6 | (a) |
| `getLocationId` | 3 | (a) |
| `new_xpath_variableNullableString` | 3 | já desambiguado por tipo, mas insuficiente |
| `getTrackStartList`, `importDouble`, `namespace_uri`, `_syntaxBridgeIterable`, `set_value_integerNullableStringIntIntIntBool` | 2 cada | mistos |

Exemplos concretos, lidos de `.diagnosis/dart-package/lib/`:

```
adjustslursfunctor.dart:641   The name 'CalcEndPointShift' is already defined.
artic.dart:108                The name 'IsInsideArtic' is already defined.
boundingbox.dart:406          The name 'GetRectangles' is already defined.
jsonxx.dart:200               bool is_() {          ← segunda definição
```

Note `new_xpath_variableNullableString` e
`set_value_integerNullableStringIntIntIntBool`: o mecanismo de desambiguação
por tipo de parâmetro **está** funcionando ali (o nome carrega a assinatura) e
mesmo assim colide — sinal de que dois overloads têm a mesma assinatura depois
do mapeamento de tipos (por exemplo, `const char*` e `char*` viram os dois
`String?`).

## Onde mexer

- `crates/server/src/function_catalog.rs` — `apply_overload_renames` (~469) e
  a construção da chave de identidade que ele usa. `declaration_identity`
  (mesmo arquivo) também é candidata: se ela considerar `const` e não-`const`
  a mesma declaração, o merge já perde um dos dois antes de chegar aqui.
- `crates/server/src/mapping.rs` — `overload_options_for`, que decide *como*
  renomear.
- `crates/server/src/lower/cpp.rs` — `dart_operator_bridge_name` (~6285), para
  a lacuna (b). O doc comment diz que o nome é "shared with the emitter so
  every declaration and call site keeps the same target" — qualquer sufixo novo
  precisa ser derivado da mesma forma nos dois lados, senão declaração e
  chamada divergem.

A direção: incluir na chave de desambiguação (i) `const`-ness do método
(`clang_CXXMethod_isConst`), (ii) o tipo dos parâmetros **depois** do mapeamento
para Dart — porque é a colisão em Dart que importa, não em C++ — e (iii) para
os nomes-ponte de operador, o tipo do operando.

Se depois disso dois membros continuarem colidindo, a última defesa é um sufixo
ordinal estável e determinístico (nunca dependente da ordem de visitação, que
varia entre workers). Colisão silenciosa não é aceitável: o Dart não compila.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro, caso (a): uma classe C++ com
   `int GetX();` e `int GetX() const;`, ambos com corpo. Verifique que os dois
   aparecem no Dart emitido com nomes distintos, e que as chamadas apontam para
   o certo.
2. Teste que falha, caso (b): duas sobrecargas de `operator<<` no mesmo arquivo
   C++. Verifique que os dois nomes-ponte diferem e que as chamadas resolvem.
3. Teste do caso residual: dois overloads cujos tipos de parâmetro **colidem
   depois do mapeamento** (`const char*` e `char*`). Verifique que o resultado
   compila.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `duplicate_definition` → **zero**. É um erro puramente mecânico: qualquer
  resíduo é um caminho não coberto.
- Nenhum `code` novo. Os riscos específicos: `undefined_method` /
  `undefined_function` subindo, se a renomeação alcançar a declaração mas não
  todos os call sites — essa é a falha típica desta classe de mudança, e o
  teste 1 acima existe para pegá-la.
- A contagem total de erros não pode subir.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível: o esquema de nomes. Hoje o
corpus tem tanto `GetOptionsConst` (sufixo semântico legível) quanto
`set_value_integerNullableStringIntIntIntBool` (assinatura concatenada,
ilegível). Se a correção exigir escolher **um** esquema para o produto inteiro,
isso muda de forma observável todo o Dart gerado e é decisão do usuário —
pergunte. Recomendação: sufixo semântico curto quando a diferença for
`const`-ness ou aridade, assinatura só como último recurso.

Dificuldade técnica não é motivo para parar.
