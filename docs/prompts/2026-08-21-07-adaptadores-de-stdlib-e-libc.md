# Tarefa 07 — Adaptadores para funções livres da stdlib, e fronteira externa para libc

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido; quando não houver
equivalente direto em Dart, a resposta é "uma fronteira/adaptador nomeado e
explícito, não um apagamento do tipo").

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F6**. Este prompt é autocontido.

## A causa raiz

O pipeline já tem um caminho para **métodos** da stdlib:
`lower::cpp::lower_stdlib_method_call` (`crates/server/src/lower/cpp.rs`, por
volta da linha 6870) mapeia `vector::size` → `.length`, `vector::empty` →
`.isEmpty`, e dezenas de outros pares `(template, método)`.

Não existe caminho equivalente para as **funções livres**. Uma chamada a
`std::max(a, b)` cai no fallback genérico de construção de `ir::Expr::Call`,
que aceita o nome porque `is_plain_dart_identifier("max")` é verdadeiro
(`cpp.rs`, ~6273), e o emissor imprime `max(a, b)` literalmente. Como isso
quase sempre está dentro de um método, o Dart lê como `this.max(a, b)` — daí o
erro sair como `undefined_method`, e não como `undefined_function`.

Para libc/POSIX o mesmo acontece, mas a resposta certa é diferente: `memset`,
`fclose`, `malloc` não são stdlib do C++ com equivalente Dart — são **fronteira
externa**, e o produto já tem esse conceito
(`docs/plans/lista-de-externos.md`, `crates/server/src/externals.rs`).

## A evidência

### Metade A — `std::` sem adaptador

Todas em `.diagnosis/dart-package/lib/`:

```dart
accid.dart:193          horizontalMargin = max(horizontalMargin, value);
devicecontext.dart:131  return make_pair(_m_baseWidth, _m_baseHeight);
adjustslursfunctor.dart:300  return pair(0, 0);
adjustarpegfunctor.dart:54   … min(…)
adjustarticfunctor.dart:94   … abs(…)
accid.dart:471               … to_string(…)
calcstemfunctor.dart:99      … vector(…)
```

Contagens, extraídas de `.diagnosis/verovio-6.2.0.analyze.json` por nome de
método não encontrado:

| nome | n |
| --- | ---: |
| `max` | 116 |
| `pair` | 84 |
| `to_string` | 75 |
| `vector` | 61 |
| `min` | 52 |
| `abs` | 51 |
| `make_pair` | 46 |
| outros (`make_tuple`, `find_if`, `sort`, `swap`, …) | resto |

Mais 286 `undefined_identifier 'basic_string'` — de `std::string(x)` escrito
com sintaxe de construção funcional.

### Metade B — libc/POSIX

`undefined_function`, **158 ocorrências** em 6 arquivos
(`zip_file.dart` 70, `pugixml.dart` 50, `vrv.dart` 19, `tuningsimpl.dart` 8,
`jsonxx.dart` 6, `main.dart` 5):

```dart
zip_file.dart:1187   free(pComp);
```

`memset` (16), `memcpy` (12), `timespec` (9), `fclose` (7), `ftello64` (7),
`free` (6), `malloc` (5), `fseeko64` (5), `__builtin_expect` (4), `allocate`
(4), `deallocate` (5)…

Junto vêm tipos do mesmo mundo, em `undefined_class` (59 no total):
`_IO_FILE` (18), `stat` (2), `stat64`, `tm`, `timeval`, `utimbuf`,
`mz_internal_state`, `locale`. E em `non_type_as_type_argument` (19):
`__va_list_tag` (17), em `vrv.dart:30` → `List<__va_list_tag> args = List.empty();`.

Também nesta família: `non_type_in_catch_clause` (1, `iomei.dart:8107` →
`on invalid_argument catch`) e `nullable_type_in_catch_clause` (2,
`iocmme.dart:152` → `on String? catch`) — exceções da stdlib do C++ sem tipo
Dart correspondente.

Total atribuído: ~**1.201** ocorrências (4,8% do relatório).

## Onde mexer

### Metade A

- `crates/server/src/lower/cpp.rs` — criar o análogo de
  `lower_stdlib_method_call` para funções livres, consultado no caminho de
  `lower_call_expr` **antes** do fallback genérico. Reconheça pelo `usr`/pelo
  namespace do cursor referenciado, não só pelo nome: `max` pode ser uma função
  do próprio projeto.

Mapeamentos diretos (Dart 3, `dart:math` para `max`/`min`):

| C++ | Dart |
| --- | --- |
| `std::max(a, b)` / `std::min(a, b)` | `math.max(a, b)` / `math.min(a, b)` |
| `std::abs(x)` | `x.abs()` |
| `std::to_string(x)` | `x.toString()` |
| `std::make_pair(a, b)` / `std::pair<A,B>(a, b)` | `SyntaxBridgePair(a, b)` (já existe em `syntax_bridge_support.dart`) |
| `std::string(x)` / `std::basic_string(x)` | `x` (a conversão já está representada) |
| `std::swap(a, b)` | troca via temporário, ou um helper nomeado no arquivo de suporte |
| `std::find_if(v.begin(), v.end(), p)` | ver **tarefa 13** — o idioma inteiro, não a função isolada |

Se `math.max` exigir um import de `dart:math`, o cálculo de imports por arquivo
em `emit::dart` precisa aprender a incluí-lo (mesmo mecanismo que a tarefa 06
toca para o arquivo de suporte).

O que **não** tiver mapeamento honesto deve virar `Expr::UnsupportedTyped` com
o tipo do contexto — nunca uma chamada literal a um nome que não existe, que é
o comportamento de hoje.

### Metade B

Isto é **categoria "mais informação na ingestão"**. `crates/server/src/externals.rs`
e `docs/plans/lista-de-externos.md` descrevem o mecanismo de detecção
automática de símbolos que o projeto declara e nunca define. Um símbolo vindo
de um header de sistema (`clang_Location_isInSystemHeader` já é consultado em
`function_catalog.rs`, por volta de 2426) que é *chamado* mas nunca definido
pelo projeto é exatamente um externo — e o pipeline hoje não o classifica assim
quando ele vem de libc.

A saída para cada um é uma fronteira nomeada e explícita: um adaptador declarado
(que pode lançar `UnimplementedError` até alguém implementá-lo) com a assinatura
certa, importado, e visível na lista de externos da UI. Nunca uma chamada solta.

## Método

TDD, conforme `AGENTS.md`. Faça as duas metades como incrementos separados:

1. **A** — teste que falha: um `.cpp` que chama `std::max`, `std::abs` e
   `std::to_string`; verifique o Dart emitido. Veja
   `crates/server/tests/lower_cpp.rs` para o padrão de fixture.
2. **B** — teste que falha: um `.cpp` que chama `memset` e usa `FILE*`;
   verifique que o símbolo aparece na lista de externos e que o Dart emitido
   referencia a fronteira nomeada, não `memset` solto.
3. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `undefined_function` — de **158** para **zero**.
- `undefined_method` — queda de ~688 (o resto das 8.309 é de outras tarefas,
  principalmente a 01).
- `undefined_identifier` — queda de ~355 (`basic_string` e afins).
- `undefined_class` — queda dos ~30 que são tipos de libc; os ~28 de
  `__normal_iterator` são da tarefa 13.
- `non_type_as_type_argument` (19), `non_type_in_catch_clause` (1),
  `nullable_type_in_catch_clause` (2) → zero.
- Nenhum `code` novo. Atenção: `math.max(a, b)` em Dart tem regras de tipo
  próprias com `num`/`int`/`double`; se `argument_type_not_assignable` ou
  `invalid_assignment` subirem, é aí.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível é a metade B: uma fronteira
externa para `malloc`/`free`/`memcpy` só faz sentido se houver um modelo de
memória do outro lado, e o produto pode preferir declarar que código que faz
gerência manual de memória **não é transpilável** e produzir um bailout de
statement honesto, em vez de uma fronteira que ninguém vai conseguir
implementar. As duas são defensáveis, mudam o produto de forma observável, e a
escolha é do usuário. `zip_file.dart` e `pugixml.dart` — os dois arquivos mais
afetados — são exatamente esse tipo de código.

Dificuldade técnica não é motivo para parar.
