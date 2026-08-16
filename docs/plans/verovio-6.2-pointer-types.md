# Tipos de ponteiro do Verovio 6.2.0 — trivialidade de mapeamento

Continuação do diagnóstico em `docs/plans/diagnostico-verovio-6.2.0.md`, com
foco estreito em um único achado daquele documento (achado 5, ponteiros
brutos): dado que um `T*` cujo pointee é um `struct`/`class` do projeto já
mapeia hoje para `T?` (`mapping::pointer_options_for`, caso A10), quantos dos
16070 ponteiros catalogados em `crates/server/tests/fixtures/verovio/
verovio-version-6.2.0.tar.gz` já caem nesse caso — e, para os que não caem,
o que falta para que caiam.

## Metodologia

1. Reimport do fixture via `sb init` (CLI, `docs/plans/interface-de-linha-de-comando.md`)
   apontando para o tarball do Verovio 6.2.0, seguido de `sb pointers --json`
   contra o `project.db` resultante — o mesmo catálogo que
   `project_service::list_pointers` expõe.
2. Cada um dos 16070 `pointer_declarations` foi classificado cruzando
   `pointee_usr`/`pointee_type_name` com `type_declarations.kind` (quando
   `pointee_usr` não é vazio) e, para os `typedef`s mais frequentes, com o
   header real do fixture (conferido por grep, não assumido).

## Resultado agregado

| Categoria | Ponteiros | % |
| --- | ---: | ---: |
| Já trivial — classe/struct do projeto (`T?`) | 14175 | 88,2% |
| Quase-trivial — `char_t` (`typedef char`, `pugi::char_t`) | 412 | 2,6% |
| Quase-trivial — `char`/`const char` cru | 280 | 1,7% |
| Opaco — `void*` | 248 | 1,5% |
| Typedef de struct anônima (`mz_zip_archive`, `tdefl_compressor`, ...) | 227 | 1,4% |
| Opaco — escalar numérico cru | 159 | 1,0% |
| Opaco — typedef de escalar (miniz: `mz_uint8/16/32/64`, ...) | 126 | 0,8% |
| Já trivial — `std::string` (E05) | 107 | 0,7% |
| Opaco — tipo externo sem `usr` (`FILE`, `struct tm`, `ELEMENT`, ...) | 96 | 0,6% |
| Typedef de `std::vector<T*>` (`ArrayOf*`, `ChordNoteGroup`) | 83 | 0,5% |
| `T**` (múltiplos níveis) | 56 | 0,3% |
| Typedef não verificado manualmente | 39 | 0,2% |
| Typedef de `std::list<T*>` (`ListOf*`) | 21 | 0,1% |
| Já trivial — `std::vector<T>` (E05) | 14 | 0,1% |
| Parâmetro de template não resolvido | 10 | 0,1% |
| `usr` sem declaração correspondente | 6 | 0,0% |
| `enum` do projeto | 4 | 0,0% |
| Typedef de `std::map<K, V>` (`MapOfStrOptions`) | 4 | 0,0% |
| Typedef de `std::set<T*>` (`SetOfConstObjects`) | 3 | 0,0% |

88% já eram triviais antes deste documento — o mecanismo central
(`mapping::pointer_options_for` caso A10, `lower::cpp::lower_type`) já
escalava para qualquer `struct`/`class`, com qualquer grau de fan-out
polimórfico (`LinkingInterface::GetNextLink()` enumera ~150 subclasses e
ainda assim vira `T?`, sem bridge). Cinco achados nomeáveis, sobre o
restante, motivaram os casos abaixo.

## Casos

### Caso 1 — catálogo de ponteiros não desfaz `typedef` (achado, não corrigido nesta rodada)

`pointer_catalog::record_pointer` (antes deste documento) resolvia o pointee
via `type_catalog::resolve_named_declaration`, que para `Alca*` (onde `Alca`
é `typedef struct {...} Alca;`, o idioma clássico de C) parava no primeiro
`clang_getTypeDeclaration` — a própria declaração do `typedef` — sem nunca
desfazê-la para a struct anônima real. Achado incidental que motivou o caso 2
abaixo.

### Caso 2 — `pointee_usr` não desfazia `typedef` (corrigido)

**Verificado antes de corrigir**: `lower::cpp::lower_type` já desfaz
`typedef` corretamente (`CXType_Typedef`, unwrap recursivo) — um teste real
(`crates/server/tests/lower_cpp.rs`,
`a_pointer_to_a_typedef_of_an_anonymous_struct_becomes_a_nullable_reference`)
confirma que `Alca*` já virava `Type::Nullable(Record)` mesmo antes de
qualquer mudança. O problema era só de exibição/catálogo, não de geração de
código.

**Correção**: `pointer_catalog::desugar_typedefs` (nova função) segue a
cadeia de `typedef` até o tipo real antes de `resolve_named_declaration` ser
chamado, igualando o que o catálogo reporta ao que `lower_type` já enxerga.
Efeito colateral corrigido na mesma direção oposta: um `typedef` que desazuça
para um escalar (`mz_uint8` = `unsigned char`) agora corretamente fica com
`pointee_usr` vazio — antes, `list_pointers` (que não checa `kind`, só se
`pointee_usr` é não-vazio) teria tratado esse ponteiro como trivial por
engano.

Testes: `crates/server/tests/pointer_catalog.rs`,
`pointee_usr_desugars_through_a_typedef_of_an_anonymous_struct` e
`pointee_usr_stays_empty_when_a_typedef_desugars_to_a_scalar`.

### Caso 3 — `char`/`const char*` não chegava a `lower_type` (corrigido)

`mapping::scalar_pointee_dart_type` já existia e decidia `char`/`const char`
→ `String`, mas só estava ligado à exibição do catálogo
(`project_service::list_pointers`), nunca ao gerador de código real: o
`match` de `CXType_Pointer` em `lower::cpp::lower_type` não tinha nenhum
braço para escalar, então caía no catch-all `Unsupported`.

**Correção**: o pointee de um `char*`/`const char*` cru é reescrito para
`ir::Type::Str` antes da decisão de forma (`PointeeShape`) — reaproveitando a
mesma representação que `std::string` já usa (E05), então cai no mesmo braço
`Known` sem precisar de um caso paralelo. `char_t` do pugixml (confirmado
como `typedef char char_t;` para o build usado pelo Verovio) se beneficia
automaticamente por já resolver como `char` uma vez que o caso 2 desfaz o
`typedef`.

**Achado incidental durante a correção**: `(void)fmt;` (o stub de
`LogDebug` do E13,
`examples/E13-fatia-real-verovio/input/src/fraction.cpp`) usava um pointee
que virou representável — a função inteira parou de abortar por parâmetro
não suportado e passou a tentar rodar o corpo, que fazia um cast-to-`void`
de um valor agora representável. `lower_expr`'s `is_transparent_wrapper` não
tinha braço para "descarte explícito via `(void)`", então gerava
`Expr::Unsupported` (`"unsupported implicit conversion from Nullable(Str)
to Void"`). Corrigido no mesmo commit: um cast para `void` de um operando
representável agora descarta o valor (`inner`), em vez de virar
`Unsupported` — C++ só alcança um cast para `void` de forma explícita
(nunca por conversão implícita), então isso é seguro incondicionalmente.
`examples/E13-fatia-real-verovio/expected/lib/fraction.dart` foi re-abençoado
(`just examples-bless`): `LogDebug` agora emite `void LogDebug(String? fmt)
{ fmt; }`, um no-op real, em vez de lançar `UnimplementedError`.

Testes: `crates/server/tests/lower_cpp.rs`,
`a_pointer_to_char_becomes_a_nullable_string` e
`a_void_cast_discards_a_representable_operand_instead_of_becoming_unsupported`;
`crates/server/tests/emit_dart.rs`,
`a_nullable_str_return_type_emits_as_a_nullable_dart_string`.

### Caso 4 — `enum` não tinha representação nenhuma no IR (corrigido)

Escopo bem maior do que os outros três: antes desta correção, não existia
`ir::Type::Enum`, `Module::enums`, nem emissão de `enum` Dart — um `enum` C++
era sempre `Type::Unsupported`, por valor ou por ponteiro, porque não havia
nada em `crate::ir` capaz de representá-lo, não só uma lacuna na decisão de
ponteiro.

**Correção** (degrau completo, não um bridge parcial):

- `ir::Type::Enum { usr, name }` (mesma forma de `Type::Record`) e
  `ir::Enum { name, usr, variants, origin }` (equivalente a `ir::Record`, um
  nível mais simples — sem campos/métodos/base).
- `lower::cpp::lower_enum` lowera a definição (`CXCursor_EnumDecl`) coletando
  cada `CXCursor_EnumConstantDecl` filho, na ordem declarada.
- `function_catalog::visit_cursor` passa a despachar `CXCursor_EnumDecl` para
  `lower_enum`, ao lado do despacho já existente de
  `StructDecl`/`ClassDecl` para `lower_record`.
- `lower::cpp::lower_type` reconhece `CXType_Enum` (braço próprio, mais
  simples que o de `Record` — sem checagem de `stdlib_template_name`/union).
- `mapping::PointeeShape::Known` passa a aceitar `ir::Type::Enum` do mesmo
  jeito que `ir::Type::Record` — um `enum*` tem a mesma garantia estática de
  conjunto finito que um `struct*`/`class*` já tinha.
- `qualified_static_member_name` reconhece `CXCursor_EnumConstantDecl` (além
  do já existente `CXCursor_VarDecl` de campo estático), qualificando
  `EnumName.valor` mesmo para uma referência não-qualificada no C++ de
  origem (`enum` não-`class` permite acesso sem prefixo; Dart sempre exige
  `EnumName.valor`).
- `emit::dart::emit_enum` emite `enum Nome { a, b, c }` de verdade —
  necessário para que `T?`/`T` referenciando o enum aponte para um tipo Dart
  que realmente existe no pacote gerado, não só para a decisão de tipo mudar.
- Persistência: nova tabela `ir_enums` (mesmo formato chave-USR/`data` JSON
  de `ir_records`/`ir_functions`), `ProjectStore::replace_ir`/`list_ir`
  passam a incluir enums, `ProjectCatalogs::ir_enums` — sem isso,
  `project_service::transpile_project` (que lê o IR persistido em vez de
  reanalisar) perderia todo enum silenciosamente ao reabrir um projeto já
  criado.

Testes: `crates/server/tests/lower_cpp.rs`,
`an_enum_lowers_by_value_as_a_pointer_and_its_constants_resolve_qualified`;
`crates/server/tests/emit_dart.rs`,
`an_enum_emits_as_a_plain_dart_enum_declaration`;
`crates/server/src/persistence/project_store.rs` (`mod tests`),
`round_trips_ir_enums`.

**Fora de escopo deste degrau**: nenhum fixture ainda usa um enumerador com
valor explícito (`enum { A = 10 }`) nem um `enum class` com tipo subjacente
diferente do padrão — `lower_enum` capta só o nome de cada variante, não seu
valor numérico. Como nenhum caso de uso deste corpus depende do valor
inteiro por trás do enumerador (sempre comparado/armazenado pelo próprio
valor simbólico), isso não é uma lacuna silenciosa — é simplesmente uma
extensão futura, do mesmo jeito que `Type::List`'s elemento genérico
(`docs/plans/primeiro-corte-e01-e03.md`) só foi exercitado com `T = Int` até
hoje.

### Caso 5 — `std::list`/`std::set`/`std::map` fora do adaptador E05 (corrigido)

`ArrayOf*`/`ChordNoteGroup` (Verovio, `include/vrv/vrvdef.h`) são
`std::vector<T*>`, já cobertos pelo caso 2 uma vez que o `typedef` é
desfeito. `ListOf*` é `std::list<T*>`, `SetOfConstObjects` é
`std::set<const Object*>`, `MapOfStrOptions` é `std::map<std::string,
Option*>` — nenhum dos três tinha adaptador (E05 só cobriu
`std::string`/`std::vector`).

**Correção**:

- `ir::Type::Set(Box<Type>)`, `ir::Type::Map(Box<Type>, Box<Type>)` — novas
  variantes, mesma forma de `Type::List`.
- `std::list<T>` reaproveita `Type::List` (não ganha variante própria): para
  a decisão de mapeamento de ponteiro, o que importa é só "este pointee já é
  representável", e ambos os containers acabam em Dart `List<T>` de qualquer
  jeito — a diferença de desempenho (`std::list` é lista ligada) é uma lacuna
  aceita do mesmo tipo que o overflow de `int` do E01, não uma escolha
  errada.
- `lower_type`'s `stdlib_template_name` passa a reconhecer `"list"`/`"set"`/
  `"map"`, além de `"vector"`/`"basic_string"` já existentes.
- `PointeeShape::Known` ganha braços para `Set`/`Map` (`List` já cobria
  `std::vector`/`std::list` juntos).
- `emit::dart::emit_type` emite `Set<T>`/`Map<K, V>` — os equivalentes
  diretos de `dart:core`, sem adaptador necessário (ao contrário de
  `std::string`/`std::vector`, que precisaram de `Type::Str`/`Type::List`
  por causa de métodos como `.size()` que não são um `FieldAccess`/`Call`
  comum).

Testes: `crates/server/tests/lower_cpp.rs`,
`std_list_set_and_map_lower_to_their_own_ir_types_by_value_and_as_nullable_pointers`;
`crates/server/tests/emit_dart.rs`,
`set_and_map_types_emit_as_their_dart_core_equivalents`.

**Fora de escopo**: só a decisão de tipo (o ponteiro/valor deixa de ser
`Unsupported`) — nenhum método de `std::list`/`std::set`/`std::map` ganhou
tradução própria (nem `std::vector` tinha cobertura completa de métodos
antes disso). Uma chamada a `.insert(...)`/`.begin()`/etc. sobre um desses
tipos continua honesta: `Unsupported` no ponto de chamada, não no tipo.

## O que ficou de fora

- **`union`** — recusado deliberadamente (`lower::cpp`, decisão já tomada,
  ver comentário em `lower_type`'s braço `CXCursor_UnionDecl`), não uma
  lacuna deste documento.
- **`T**`/ponteiro de função** — fora de escopo (`PointerShape::DoublePointer`/
  `FunctionPointer`), nenhum dos cinco casos os toca.
- **Tipos externos sem `usr`** (`FILE`, `struct tm`, `ELEMENT`, miniz's
  `mz_uint8`/`mz_zip_internal_state` e afins) — cada um precisaria de uma
  ponte própria (`dart:ffi` ou wrapper manual), caso a caso; nenhum é
  Verovio-específico o bastante, nem frequente o bastante nesta base, para
  justificar construir agora.
