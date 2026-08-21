# Tarefa 03 — `protected` do C++ não é `_` do Dart

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido, mapeamento de tipos é o
objetivo central do produto).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F2**. Este prompt é autocontido.

**Execute a tarefa 01 antes desta**, se ainda não tiver sido feita: ela muda a
contagem de `unused_field`, que é um dos alvos aqui.

## A causa raiz

`lower::cpp::dart_member_name` (`crates/server/src/lower/cpp.rs`, por volta da
linha 1126) traduz visibilidade C++ prefixando `_`:

```rust
let access = unsafe { clang_sys::clang_getCXXAccessSpecifier(cursor) };
let is_private = matches!(access, CX_CXXPrivate | CX_CXXProtected);
if is_private {
    format!("_{}", cpp_name.trim_end_matches('_'))
} else {
    dart_safe_identifier(&cpp_name)
}
```

`private` e `protected` são tratados igual. Isso está certo para `private`, mas
**errado para `protected`**, por causa de uma diferença fundamental entre as
duas linguagens:

- Em C++, `protected` quer dizer exatamente "visível para as subclasses".
- Em Dart, `_` quer dizer **privado de biblioteca**, não privado de classe. E
  cada registro é emitido no seu próprio arquivo (`emit::dart::emit_file`) —
  ou seja, na sua própria biblioteca.

Resultado: um membro `protected` fica invisível justamente para quem em C++
tinha direito de vê-lo. E, do outro lado, o campo que só a própria biblioteca
enxerga passa a nunca ser lido ali, virando `unused_field`.

## A evidência

No pacote Dart emitido do Verovio 6.2.0 (`.diagnosis/dart-package/lib/`):

`Doc? _m_doc = null;` é declarado em `functor.dart:65` (classe base
`DocFunctor`) e lido em dezenas de subclasses, cada uma no seu arquivo:

```
alignfunctor.dart:138:    int graceAlignerId = _m_doc!.GetOptions()!…
adjustarpegfunctor.dart:66:  … _m_doc …
castofffunctor.dart:317:     _m_doc!.GetPages()!.AddChild(_m_currentPage);
```

`dart analyze` (`.diagnosis/verovio-6.2.0.analyze.json`) reporta
`Undefined name '_m_doc'` **490 vezes**.

Outros casos do mesmo padrão:

| Membro | declarado em | `code` | n |
| --- | --- | --- | ---: |
| `__root` (de `_root`, campo de `xml_node`) | `pugixml.dart:152` | `undefined_getter` | 456 |
| `_m_editInfo` | `editortoolkit*.dart` | `undefined_identifier` | 244 |
| `_m_type` / `_m_px` / `_m_vu` (de `data_MEASUREMENTSIGNED`) | `attalternates.dart` | `undefined_getter` | 159 |
| `_top` / `_bot` (de `HumNum`) | `humlib.dart` | `undefined_getter` | 64 |
| `_value_map` / `_odd` (de `JsonxxObject`) | `jsonxx.dart` | `undefined_getter` | 60 |
| `_m_numerator` / `_m_denominator` (de `Fraction`) | `fraction.dart` | `undefined_getter` | 38 |

Exemplo concreto de uma linha só: `.diagnosis/dart-package/lib/bboxdevicecontext.dart:371`
→ `svg = xml_node(svg.__root);`, com `__root` declarado em `pugixml.dart`.

Totais atribuídos a esta família: **1.169** diretos
(`undefined_identifier` 754 + `undefined_getter` 415), mais uma parte
significativa dos **1.759** `unused_field` e alguns `undefined_method` de
métodos `protected`.

## Onde mexer

- `crates/server/src/lower/cpp.rs`, `dart_member_name` — o ponto único onde a
  decisão de nome é tomada. O doc comment de `ir::Field::name`
  (`crates/server/src/ir/mod.rs`, por volta de 740) explica por que esse ponto é
  único: "a field's declaration and every access of it can never disagree on
  whether it's private". Mantenha essa propriedade.
- Verifique se há outros pontos que aplicam a mesma regra de visibilidade a
  métodos (não só campos) — `qualified_static_member_name` no mesmo arquivo
  delega para `dart_member_name`, e há caminhos para métodos que podem ter
  cópia da regra.

A direção da correção: distinguir `CX_CXXPrivate` de `CX_CXXProtected`.
`private` continua com `_`; `protected` vira público. Se um nome público
colidir com algo (palavra reservada do Dart, membro herdado), a máquina de
renomeação existente (`apply_reserved_word_renames` em `function_catalog.rs`) é
o lugar certo para resolver, não um `_` de volta.

O `_` de `private` também pode dar problema em um caso: uma classe C++ com
membros `private` cujos *métodos definidos out-of-line* passam a existir depois
da tarefa 01 — mas esses métodos vão para o mesmo arquivo Dart da classe, então
continuam enxergando. Se a tarefa 01 ainda não tiver rodado, meça de novo
depois dela antes de concluir que sobrou algo.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro. Fixture mínimo: uma classe C++ com um membro
   `protected` e uma subclasse, em arquivos diferentes, com a subclasse lendo o
   membro. Verifique que o Dart emitido para a subclasse referencia um nome que
   a base realmente expõe. Veja `crates/server/tests/lower_cpp.rs` para o
   padrão de fixture.
2. Um segundo teste garantindo que `private` **continua** com `_` — a correção
   não pode ser "tirar o `_` de tudo".
3. Implemente até passar.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak), comparando com `.diagnosis/verovio-6.2.0.analyze.json`:

| `code` | antes | esperado |
| --- | ---: | --- |
| `undefined_identifier` | 1223 | queda de ~754 (o resíduo é a tarefa 07: `basic_string`, `chartype_table` etc.) |
| `undefined_getter` | 920 | queda de ~415 |
| `unused_field` | 1759 | queda significativa (medir; o resto é da tarefa 01 ou C++ genuinamente morto) |

Verificação direta: `_m_doc` não pode mais aparecer em nenhum arquivo que não
seja o de `DocFunctor`; ou o nome vira público em ambos os lados.

Nenhum `code` novo. Em especial, `private_optional_parameter`,
`invalid_use_of_visible_for_testing_member` e colisões
(`duplicate_definition`) seriam sinais de que a mudança de nome quebrou algo.

## Quando parar e perguntar

Isto é uma **decisão de produto** e você deve levá-la ao usuário antes de fixar
a implementação, porque muda a forma do Dart gerado de maneira observável e
permanente:

- **Opção A** — `protected` vira membro público (`m_doc`). Simples, resolve
  tudo, mas expõe na API pública do arquivo Dart membros que em C++ eram
  internos à hierarquia.
- **Opção B** — `protected` vira público com convenção de nome que sinalize a
  intenção (por exemplo, um sufixo/prefixo documentado). Mesma resolução, com
  o custo de um nome menos natural.
- **Opção C** — manter `_` e emitir a classe base e todas as suas subclasses na
  **mesma biblioteca Dart** (um `part`/`part of`, ou um arquivo por hierarquia
  em vez de um por registro). Preserva a semântica de visibilidade, mas inverte
  o layout de arquivos do produto e não escala para hierarquias grandes — a
  hierarquia de `Object` do Verovio tem mais de 200 classes num único arquivo.

Recomendação: **A**, com a opção B só se o usuário quiser a sinalização. Mas
pergunte — as três são tecnicamente válidas e mutuamente exclusivas.

Dificuldade técnica não é motivo para parar.
