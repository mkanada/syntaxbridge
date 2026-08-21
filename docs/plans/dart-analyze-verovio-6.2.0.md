# `dart analyze` sobre o Verovio 6.2.0 — agrupamento, causas raiz e backlog

Este documento é o **backlog** dos erros que o `dart analyze` reporta sobre o
pacote Dart emitido a partir do Verovio 6.2.0 real, não modificado. Ele
complementa `docs/plans/diagnostico-verovio-6.2.0.md`, que é o log dos achados
**já corrigidos**: aqui nada foi corrigido ainda.

Cada família de causa raiz identificada abaixo tem um arquivo de prompt
correspondente em `docs/prompts/2026-08-21-NN-*.md`, na ordem de execução
sugerida (§5).

## 1. Entrada e método

Fonte: `.diagnosis/verovio-6.2.0.analyze.json` — saída bruta de
`dart analyze --format=json` sobre o pacote inteiro, gerada por
`just verovio-diagnosis` em **2026-08-21T03:09:33Z, commit `e0728a6`**
(mesmo commit que era `HEAD` quando esta análise foi escrita).

O Dart real lido nos exemplos está em `.diagnosis/dart-package/lib/`, que é o
pacote persistido daquela mesma rodada. O C++ de origem foi correlacionado
extraindo `test-resources/verovio-version-6.2.0.tar.gz` — a correspondência é
por nome de arquivo/classe, não automática: o Dart emitido não carrega
comentário de proveniência linha a linha.

O agrupamento e as contagens foram produzidos por script descartável sobre o
JSON (não faz parte do produto). Os caminhos em `location.file` apontam para um
diretório temporário que não existe mais; só o sufixo a partir de `lib/` é
identidade útil de arquivo.

> **Aviso de escopo.** A árvore de trabalho tinha, no momento desta análise,
> uma mudança **não commitada** em `crates/server/src/lower/cpp.rs`
> (suporte a `std::stack`). Ela **não** está refletida no snapshot. Nenhuma
> conclusão abaixo depende dela, mas a próxima rodada de
> `just verovio-diagnosis` vai incluí-la.

### Números da rodada

| Métrica | Valor |
| --- | --- |
| Diagnósticos totais | 24.791 |
| Erros (`ERROR`) | 15.738 |
| Avisos (`WARNING`) | 9.053 |
| `code`s distintos | 52 |
| Arquivos `.dart` emitidos | 301 |
| Arquivos com ao menos um diagnóstico | 289 |
| Unidades de compilação C++ | 298 |

## 2. Objetivo 1 — inventário por tipo de erro

Os 52 `code`s, do mais frequente ao menos, com a família de causa raiz a que
cada um foi atribuído (§4). A lista **completa** de arquivos por `code`, com a
contagem por arquivo, está no Anexo A.

| `code` | n | arquivos | sev. | família |
| --- | ---: | ---: | --- | --- |
| `undefined_method` | 8309 | 145 | E | F1 / F5 / F6 / F9 |
| `unnecessary_non_null_assertion` | 6107 | 77 | W | F3 |
| `unused_field` | 1759 | 67 | W | F1 / F2 (sintoma) |
| `argument_type_not_assignable` | 1569 | 31 | E | F7 / F9 / F11 |
| `undefined_identifier` | 1223 | 52 | E | F2 / F6 |
| `undefined_getter` | 920 | 32 | E | F2 / F1 |
| `override_on_non_overriding_member` | 750 | 194 | W | F1 |
| `not_enough_positional_arguments` | 583 | 16 | E | F1 |
| `invalid_assignment` | 564 | 61 | E | F7 / F9 / F11 |
| `extra_positional_arguments` | 481 | 65 | E | F4 / F15 |
| `extends_non_class` | 361 | 65 | E | F4 |
| `unchecked_use_of_nullable_value` | 334 | 4 | E | F9 |
| `implicit_super_initializer_missing_arguments` | 319 | 71 | E | F1 |
| `definitely_unassigned_late_local_variable` | 179 | 17 | E | F8 |
| `undefined_operator` | 161 | 14 | E | F1 |
| `undefined_function` | 158 | 6 | E | F6 |
| `unused_local_variable` | 155 | 27 | W | F8 (sintoma) |
| `unused_import` | 143 | 50 | W | F15 |
| `non_abstract_class_inherits_abstract_member` | 135 | 125 | E | F1 |
| `duplicate_definition` | 118 | 17 | E | F13 |
| `dead_code` | 83 | 14 | W | F15 |
| `undefined_class` | 59 | 15 | E | F10 / F6 |
| `non_bool_condition` | 46 | 14 | E | F14 / F9 |
| `assignment_to_final` | 39 | 5 | E | F15 |
| `return_of_invalid_type` | 35 | 24 | E | F7 / F9 |
| `non_bool_negation_expression` | 29 | 19 | E | F9 |
| `for_in_of_invalid_type` | 24 | 13 | E | F15 / F10 |
| `constant_pattern_never_matches_value_type` | 21 | 1 | W | F9 |
| `non_type_as_type_argument` | 19 | 3 | E | F10 / F6 |
| `unnecessary_null_comparison` | 18 | 3 | W | F9 |
| `undefined_setter` | 15 | 1 | E | F9 |
| `const_eval_method_invocation` | 10 | 6 | E | F15 |
| `non_bool_operand` | 9 | 6 | E | F9 |
| `map_key_type_not_assignable` | 6 | 2 | E | F15 |
| `invalid_override` | 6 | 2 | E | F9 |
| `const_eval_property_access` | 5 | 4 | E | F15 |
| `receiver_of_type_never` | 5 | 1 | W | F14 |
| `null_check_always_fails` | 5 | 1 | W | F14 |
| `unnecessary_type_check` | 4 | 2 | W | F4 |
| `referenced_before_declaration` | 3 | 2 | E | F15 |
| `not_a_type` | 3 | 2 | E | F9 |
| `positional_field_in_object_pattern` | 3 | 2 | E | F9 |
| `refutable_pattern_in_irrefutable_context` | 3 | 2 | E | F9 |
| `undefined_constructor_in_initializer` | 2 | 2 | E | F1 |
| `return_of_invalid_type_from_closure` | 2 | 2 | E | F9 |
| `pattern_type_mismatch_in_irrefutable_context` | 2 | 1 | E | F9 |
| `nullable_type_in_catch_clause` | 2 | 2 | W | F6 |
| `non_type_in_catch_clause` | 1 | 1 | E | F6 |
| `invocation_of_non_function_expression` | 1 | 1 | E | F15 |
| `main_first_positional_parameter_type` | 1 | 1 | E | F9 |
| `missing_assignable_selector` | 1 | 1 | E | F15 |
| `unused_element` | 1 | 1 | W | F1 (sintoma) |

### Concentração

O agrupamento por família (§4) atribui as 24.791 ocorrências assim — a
atribuição é heurística (por `code` + padrão da `problemMessage`), não uma
prova, e serve para priorizar, não para prometer contagens exatas de queda:

| Família | Ocorrências | % |
| --- | ---: | ---: |
| **F1** — registro truncado (métodos perdidos no merge entre TUs) | 9.474 | 38,2% |
| **F3** — `!` incondicional, ignorando promoção de tipo do Dart | 6.107 | 24,6% |
| **F1/F2** — `unused_field` (sintoma das duas) | 1.759 | 7,1% |
| **F7** — downcast de hierarquia apagado | 1.523 | 6,1% |
| **F9** — bailout tipado / `SyntaxBridgeOpaque` vazando | 1.276 | 5,1% |
| **F6** — chamadas livres de `std::`/libc sem adaptador | 1.201 | 4,8% |
| **F2** — privacidade Dart é por biblioteca, não por classe | 1.169 | 4,7% |
| **F4** — `mixin` vs. `class` decidido sem visão do projeto | 842 | 3,4% |
| **F5** — `assignFrom` (atribuição por valor) nunca materializado | 483 | 1,9% |
| **F8** — parâmetro de saída por referência (`T&`) | 179 | 0,7% |
| **F11** — aritmética `int`/`double` | 153 | 0,6% |
| **F13** — overloads que colidem no mesmo nome Dart | 118 | 0,5% |
| **F10** — iteradores STL | 44 | 0,2% |
| **F14** — parentização/precedência na emissão | ~55 | 0,2% |
| **F15** — resíduos de emissão (limpeza) | ~450 | 1,8% |
| Não atribuído | ~168 | 0,7% |

## 3. Objetivo 2 — o que cada `code` está reclamando

Descrição em português direto, com um exemplo real lido de
`.diagnosis/dart-package/lib/`. A causa **não** é discutida aqui — isso é §4.

### Os oito mais frequentes

**`undefined_method` (8309)** — o Dart não achou o método que o código está
chamando na classe do receptor. Em `accid.dart:122` o corpo de `Accid.Reset()`
chama `ResetColor();`, mas nenhuma das classes/mixins de `Accid` declara
`ResetColor`. Quando a chamada não tem receptor explícito (como aqui), o Dart a
lê como `this.ResetColor()` — por isso funções livres não encontradas também
aparecem sob este `code`, e não sob `undefined_function`.

**`unnecessary_non_null_assertion` (6107)** — o `!` (que quer dizer "confio que
isso não é nulo") foi escrito num valor que o Dart **já sabe** não ser nulo. Não
quebra nada; é ruído. Em `adjustbeamsfunctor.dart:196`, `if (rest!.GetDots() > 0)`
vem depois de um `rest!` anterior no mesmo fluxo, e o Dart já promoveu `rest`
para não-nulo.

**`unused_field` (1759)** — um campo privado (`_algo`) é declarado e nunca
**lido** dentro do arquivo em que foi declarado. `humlib.dart:682` declara
`int _m_tpq;` e nada naquele arquivo o consulta.

**`argument_type_not_assignable` (1569)** — o argumento passado tem um tipo que
não cabe no parâmetro. `humlib.dart:3059`: `assignFrom(duration, -1);` passa um
`HumNum` onde se esperava um `MSearchQueryToken`.

**`undefined_identifier` (1223)** — um nome solto (nem método, nem getter de um
objeto: um identificador) não existe no escopo. `dynam.dart:314`:
`to = dynamSmufl[i];`, e `dynamSmufl` não está declarado em lugar nenhum
visível.

**`undefined_getter` (920)** — o Dart não achou o campo/propriedade sendo lido.
`bboxdevicecontext.dart:371`: `svg = xml_node(svg.__root);` — `__root` existe em
`pugixml.dart`, mas não é enxergável de `bboxdevicecontext.dart`.

**`override_on_non_overriding_member` (750)** — o método está marcado
`@override`, mas nenhuma superclasse/mixin declara um método com esse nome, então
não há o que sobrescrever. `abbr.dart:29`: `@override void Reset()`, e nenhum dos
mixins de `Abbr` declara `Reset`.

**`not_enough_positional_arguments` (583)** — a chamada passou menos argumentos
posicionais do que a assinatura exige. `editortoolkit_neume.dart:434`:
`Nc? nc = Nc();`, mas o construtor de `Nc` gerado exige 1 argumento.

### O restante

- **`invalid_assignment` (564)** — o valor atribuído não cabe na variável.
  `calcstemfunctor.dart:474` → `note = parent;` (`LayerElement` numa `VrvNote?`).
- **`extra_positional_arguments` (481)** — a chamada passou argumentos
  posicionais **a mais**. `iocmme.dart:153` → `LogError('%s', str);` para uma
  `LogError` que aceita 1.
- **`extends_non_class` (361)** — `extends X` onde `X` não é uma `class` (aqui,
  é sempre um `mixin`). `atts_shared.dart:3689` → `class AttXy extends Att {`,
  com `Att` declarado `mixin`.
- **`unchecked_use_of_nullable_value` (334)** — usou um valor possivelmente nulo
  sem `?.`, `!` ou checagem. `zip_file.dart:681` → `++pByte_buf;`.
- **`implicit_super_initializer_missing_arguments` (319)** — a subclasse tem
  construtor sem `super(...)`, mas o construtor sem-nome da base exige
  parâmetros. `atts_critapp.dart:19` → `InstCrit();`.
- **`definitely_unassigned_late_local_variable` (179)** — leu uma variável
  `late` que comprovadamente ainda não recebeu valor. `zip_file.dart:988` →
  `return ret;`, com `late int ret;` nunca atribuído.
- **`undefined_operator` (161)** — o tipo não define esse operador.
  `iohumdrum.dart:1736` → `if (sum > 0)` com `sum` do tipo `HumNum`, que não
  declara `operator >`.
- **`undefined_function` (158)** — função livre inexistente, chamada fora de
  qualquer classe. `zip_file.dart:1187` → `free(pComp);`.
- **`unused_local_variable` (155)** — variável local declarada e nunca lida.
  `pugixml.dart:2723` → `int ui = 1;`.
- **`unused_import` (143)** — `import` de um arquivo do qual nada é usado.
  `calcalignmentpitchposfunctor.dart:13` → `import 'nc.dart';`.
- **`non_abstract_class_inherits_abstract_member` (135)** — a classe é concreta
  mas herda métodos sem corpo que ela não implementa. `options.dart:189` →
  `class OptionDbl extends Option {` sem `CopyTo`, `GetStrValue`, `IsSet`…
- **`duplicate_definition` (118)** — dois membros com o mesmo nome no mesmo
  escopo. `jsonxx.dart:200` → um segundo `bool is_() {`.
- **`dead_code` (83)** — código que nunca executa. `vrv.dart:282` → `if (false) {`.
- **`undefined_class` (59)** — o nome usado como tipo não é uma classe conhecida.
  `zip_file.dart:265` → `_IO_FILE? m_pFile;`.
- **`non_bool_condition` (46)** — a condição de um `if`/`while` não é `bool`.
  `iohumdrum.dart:9035` → `if (solo ? 1 : 0 == true.toInt())`, que pela
  precedência do Dart é `solo ? 1 : (0 == …)` — um `int`.
- **`assignment_to_final` (39)** — tentou escrever num campo `final`.
  `iohumdrum.dart:9192` → `v.second = i;` (`SyntaxBridgePair.second` é `final`).
- **`return_of_invalid_type` (35)** — o valor devolvido não é do tipo de retorno
  declarado. `fb.dart:13` → `return Fb();` num método `VrvObject? Clone()`,
  com `Fb` não sendo (para o Dart) um `VrvObject`.
- **`non_bool_negation_expression` (29)** — `!x` com `x` não-`bool`.
  `iopae.dart:1433`.
- **`for_in_of_invalid_type` (24)** — `for (x in y)` com `y` não iterável.
  `bboxdevicecontext.dart:343` → `for (int c in text)` com `text` sendo `String`.
- **`constant_pattern_never_matches_value_type` (21)** — um `case` cujo valor
  nunca pode ser igual ao que está sendo comparado. `pugixml.dart:1782` →
  `case 98:` sobre um `switch` de `SyntaxBridgeOpaque`.
- **`non_type_as_type_argument` (19)** — usou algo que não é tipo como argumento
  genérico. `vrv.dart:30` → `List<__va_list_tag> args`.
- **`unnecessary_null_comparison` (18)** — comparou com `null` algo que nunca é
  nulo. `iohumdrum.dart:8859` → `if (line == null)`.
- **`undefined_setter` (15)** — escreveu num campo inexistente.
  `zip_file.dart:595` → `…m_pWrite = …` sobre um `SyntaxBridgeOpaque`.
- **`const_eval_method_invocation` (10)** — valor default de parâmetro precisa
  ser constante, e chama um método. `editortoolkit_neume.dart:2695` →
  `[double rotate = 0.toDouble()]`.
- **`non_bool_operand` (9)** — operando de `&&`/`||` não é `bool`.
  `chord.dart:283`.
- **`map_key_type_not_assignable` (6)** — chave de mapa com tipo errado.
  `alignfunctor.dart:65` → chaves `option_DURATION_EQ` num `Map<int, …>`.
- **`invalid_override` (6)** — a assinatura do override não é compatível com a
  da base. `bboxdevicecontext.dart:359` — o `SyntaxBridgeOpaque` do parâmetro é
  **outra classe** que a do mesmo nome em `devicecontext.dart`.
- **`const_eval_property_access` (5)** — leitura de propriedade num default de
  parâmetro. `devicecontext.dart:146` → `[int style = PenStyle.PEN_SOLID.value]`.
- **`receiver_of_type_never` (5)** e **`null_check_always_fails` (5)** — chamou
  algo em cima de um `null` literal. `editortoolkit_neume.dart:1835` →
  `sparent is Layer ? sparent : null!.GetCurrentClef()`, que pela precedência é
  `… : (null!.GetCurrentClef())`.
- **`unnecessary_type_check` (4)** — `is` que é sempre verdadeiro. `fig.dart:26`
  → `return this is AreaPosInterface ? this : null;` dentro de uma classe que já
  aplica `AreaPosInterface` como mixin.
- **`referenced_before_declaration` (3)** — usou uma local antes de declará-la.
  `zip_file.dart:1390` → `tm tm = tm(0, …);`, onde o nome da variável esconde o
  nome do tipo.
- **`not_a_type` / `positional_field_in_object_pattern` /
  `refutable_pattern_in_irrefutable_context` (3 cada)** — os três disparam na
  mesma linha: um `_syntaxBridgeUnsupported<…>(…)` caiu numa posição de
  *padrão* de desestruturação, onde o Dart espera um tipo, não uma chamada.
  `iomei.dart:3271`.
- **`return_of_invalid_type_from_closure` (2)** — o mesmo, dentro de uma closure.
  `iopae.dart:2071`.
- **`pattern_type_mismatch_in_irrefutable_context` (2)** — desestruturação de
  tupla com aridade/tipo errados. `pugixml.dart:3503`.
- **`nullable_type_in_catch_clause` (2)** e **`non_type_in_catch_clause` (1)** —
  `on X catch` com `X` anulável (`iocmme.dart:152` → `on String? catch`) ou não
  sendo um tipo (`iomei.dart:8107` → `on invalid_argument catch`).
- **`invocation_of_non_function_expression` (1)** — `jsonxx.dart:39` →
  `format == Format.JSON.value ? json() : xml(format)` onde `json` não é função.
- **`main_first_positional_parameter_type` (1)** — `main.dart:43` →
  `int main(int argc, SyntaxBridgeOpaque argv)`.
- **`missing_assignable_selector` (1)** — `pugixml.dart:3926` →
  `swapxpath_nodexpath_node((begin++)!, --end!);`.
- **`unused_element` (1)** — `humlib.dart:3527` → `class _VoiceInfo {` nunca
  referenciada.

## 4. Objetivo 3 — causas raiz e propostas

Cada família diz explicitamente em qual das três categorias do enunciado a
proposta se encaixa:

1. **Correção local de lowering/emissão** — o dado já está no IR.
2. **Mais informação na ingestão** — o dado não está sendo extraído do C++.
3. **Fase nova, anterior à transpilação, com visão do projeto inteiro** — a
   decisão depende de como o símbolo é *usado* em outros pontos.

---

### F1 — Registro truncado: o merge entre unidades de compilação descarta os métodos out-of-line

**Categoria: 1.** Nenhum dado novo precisa ser extraído; o merge joga fora dado
que já foi lowered corretamente.

**Sintoma.** `doc.dart` tem 167 linhas para um `Doc` cujo `.cpp` tem 2.482;
`object.dart` tem 419 para 1.684; `note.dart` 176 para 1.027. Todos os métodos
que sobreviveram estão declarados **com corpo dentro do `class`** no header.
Todo método declarado no header e **definido fora** (`bool Object::AddChild(...)`
em `src/object.cpp:848`) sumiu — `AddChild` não existe em nenhum lugar do pacote
emitido, e é chamado 409 vezes.

Por contraste, `accid.dart` (483 linhas para um `.cpp` de 371) tem
`AdjustX`, `Reset`, `AdjustToLedgerLines` — todos out-of-line. E `xml_allocator`,
declarado **dentro** de `pugixml.cpp`, saiu completo, enquanto `xml_node`,
declarado em `pugixml.hpp`, saiu com um campo e um construtor e mais nada.

**Causa raiz.** `function_catalog::extract_function_catalog`
(`crates/server/src/function_catalog.rs:261-288`) divide as 298 unidades de
compilação em N pedaços, um por worker, e cada worker constrói o seu próprio
`VisitorState` com o seu próprio `ir_records`. Dentro de um worker o mecanismo
funciona: a definição out-of-line encontra o registro já empurrado
(`function_catalog.rs:2518`, `record.methods.push(method_ir)`).

O `finish_function_catalog` então **funde os partials com "o primeiro vence"**
(`function_catalog.rs:392-396`):

```rust
for record in partial_ir_records {
    if ir_record_seen.insert(record.usr.clone()) {
        ir_records.push(record);
    }
}
```

Uma classe declarada num header compartilhado é lowered em **cada** unidade que
inclui o header — mas só ganha métodos no worker que também processou o `.cpp`
onde eles estão definidos. Como `object.h` é incluído por quase tudo, o partial
que vence é o de alguma unidade que só viu o header. A cópia rica, vinda de
`object.cpp`, é **descartada inteira**. `Accid` sobreviveu por acidente de
ordenação: `accid.cpp` calhou de ser a primeira unidade daquele worker a ver
`accid.h`.

Note que o mesmo arquivo já resolve exatamente este problema, corretamente, para
funções livres logo acima (`function_catalog.rs:372-390`): protótipo perde para
definição, em vez de "o primeiro vence".

**Erros que isto explica** — 9.474 ocorrências (38,2%):
`undefined_method` (6.972 das 8.309), `override_on_non_overriding_member` (750,
`@override` num método cuja base perdeu o método correspondente),
`not_enough_positional_arguments` (583 — o construtor real foi descartado e o
emissor sintetizou um posicional a partir de todos os campos:
`21 positional arguments expected by 'VrvMeasure.new'`), `undefined_getter`
(471), `implicit_super_initializer_missing_arguments` (319),
`non_abstract_class_inherits_abstract_member` (135 — `BoundingBox::GetDrawingX`
é virtual pura, e o override de `Object` está out-of-line em `object.cpp`),
`undefined_operator` (161 — `Fraction::operator+`, `HumNum::operator*` são todos
out-of-line). Boa parte de `unused_field` (1.759) é sintoma disto: o campo é
privado e os métodos que o liam desapareceram.

**Proposta.** Trocar o merge de registros de "primeiro vence" por **união por
`usr` de membro**: fundir `methods`, `constructors`, `static_fields` e
`destructor` de todos os partials do mesmo registro, deduplicando por `usr` do
membro e preferindo a versão *com corpo* — a mesma regra que
`ir_function_is_prototype` já aplica às funções livres. `VisitorState.ir_member_seen`
é por worker e continua correto para evitar lowering duplicado dentro do worker;
a deduplicação entre workers passa a acontecer no merge.

Prompt: `docs/prompts/2026-08-21-01-uniao-de-registros-no-merge.md`

---

### F2 — Privacidade do Dart é por biblioteca, não por classe

**Categoria: 1**, mas com uma decisão de produto embutida (ver o prompt).

**Sintoma.** `Undefined name '_m_doc'` — 490 ocorrências, em 30+ arquivos.
`_m_doc` é declarado em `functor.dart:65` (`Doc? _m_doc = null;`) e lido em
`alignfunctor.dart:138`, `adjustarpegfunctor.dart:66`, `castofffunctor.dart:317`…
Mesma coisa com `__root` (456, declarado em `pugixml.dart`, lido de `iomei.dart`,
`bboxdevicecontext.dart`), `_m_editInfo` (244), `_m_type`/`_m_px`/`_m_vu` (159, em
`data_MEASUREMENTSIGNED`), `_top`/`_bot` de `HumNum` (64).

**Causa raiz.** `lower::cpp::dart_member_name`
(`crates/server/src/lower/cpp.rs:1126`) traduz visibilidade C++ prefixando `_`:

```rust
let is_private = matches!(access, CX_CXXPrivate | CX_CXXProtected);
if is_private { format!("_{}", cpp_name.trim_end_matches('_')) } else { … }
```

Isso está correto para C++ `private`, onde nem a subclasse enxerga. Mas em Dart
o `_` é **privado de biblioteca**, e cada registro é emitido em seu próprio
arquivo (`emit::dart::emit_file`) — ou seja, sua própria biblioteca. Um membro
`protected`, que em C++ é *exatamente* "visível para as subclasses", vira em Dart
"invisível para as subclasses", porque elas moram em outro arquivo. E o membro
privado que só o próprio arquivo lê passa a nunca ser lido, virando
`unused_field`.

**Erros que isto explica** — 1.169 diretos (`undefined_identifier` 754,
`undefined_getter` 415) mais uma parte grande dos 1.759 `unused_field` e alguns
`undefined_method` de métodos `protected`.

**Proposta.** `protected` não é `private`. A tradução honesta de `protected` em
Dart, dado o layout de um arquivo por registro, é um membro **público**
(`m_doc`), possivelmente com uma convenção de nome que sinalize a intenção. A
alternativa — manter o `_` e emitir o registro e todas as suas subclasses na
mesma biblioteca — inverte o layout de arquivos do produto inteiro e não escala
para hierarquias que cruzam módulos. `private` de verdade (nunca lido fora da
própria classe) pode continuar com `_`.

Isso é uma decisão de produto (a forma do Dart gerado muda de forma
observável), e o prompt a apresenta explicitamente como tal.

Prompt: `docs/prompts/2026-08-21-03-membros-privados-entre-bibliotecas.md`

---

### F3 — `!` emitido incondicionalmente, ignorando a promoção de tipo do Dart

**Categoria: 1.**

**Sintoma.** 6.107 avisos, o `code` isolado mais frequente depois de
`undefined_method`. Em `accid.dart`:

```dart
149:  void AdjustToLedgerLines(Doc? doc, LayerElement? element, int staffSize) {
150:    Staff? staff = element!.GetAncestorStaff(StaffSearch.RESOLVE_CROSS_STAFF);
152:    int unit = doc!.GetDrawingUnit(staffSize);
153:    int rightMargin = doc!.GetRightMargin(ClassId.ACCID) * …;   // ← aviso
154:    if (element!.IsClassId(ClassId.NOTE) && chord != null && chord!…) {  // ← 3 avisos
```

Depois do `element!` da linha 150, o Dart **promove** `element` a
`LayerElement` para o resto do fluxo; o `!` da linha 154 é redundante. Idem
`doc!` (153) e `chord!` depois de `chord != null` (154).

**Causa raiz.** `emit::dart::receiver_bang`
(`crates/server/src/emit/dart.rs:2708-2714`) decide puramente pelo tipo
estático do IR:

```rust
fn receiver_bang(receiver: &Expr) -> &'static str {
    if matches!(expr_ty(receiver), Some(Type::Nullable(_))) { "!" } else { "" }
}
```

O emissor não tem nenhuma noção de fluxo, então repete o `!` em cada
dereferência. A escolha de asserção (em vez de propagar a exigência de checagem
do Dart) está bem justificada no comentário da função e **não** é o problema: o
problema é a repetição.

**Proposta.** Uma passada de fluxo mínima no emissor, por corpo de função:
rastrear quais locais/parâmetros já foram promovidos a não-nulos mais cedo no
mesmo bloco linear (por `x!`, por `x != null` numa condição que domina o uso,
por atribuição de valor não-nulo), invalidando na reatribuição, e suprimir o `!`
para esses. Não precisa reproduzir a promoção do Dart inteira — o subconjunto
"mesma sequência de statements, sem reatribuição" já derruba a maioria. Campos
(`this._m_x`) o Dart nunca promove, então esses mantêm o `!`.

São avisos, não erros: nada quebra hoje. Mas é 24,6% de todo o ruído do
relatório, e ruído em código gerado é exatamente o que
`docs/plans/estilo-de-codigo-gerado.md` existe para evitar.

Prompt: `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`

---

### F4 — `mixin` vs. `class` decidido sem visão do projeto inteiro

**Categoria: 3** — é o exemplo canônico de fase global que o enunciado cita.

**Sintoma.** `atts_shared.dart`:

```dart
 7: mixin AttAccidLog on Att { … }
19: class InstAccidLog extends AttAccidLog { }   // ← extends_non_class
…
42: class AttAnnotLog extends Att { … }          // ← Att é mixin: extends_non_class
```

361 `extends_non_class`, concentrados em `atts_*.dart` (149 só em
`atts_shared.dart`), mais 481 `extra_positional_arguments`: um registro que vira
`mixin` perde o construtor posicional sintético que todo registro com campos
ganha, mas as construções dele (`Accid(this._m_drawingUnison, …)` em
`accid.dart:52`) continuam sendo emitidas com argumentos.

**Causa raiz.** `emit::dart::mixin_usrs` (`crates/server/src/emit/dart.rs:225`)
já computa, para o módulo inteiro, quais `usr` são usados como mixin — e
`emit_record` usa isso para escolher `mixin` em vez de `class`. Mas a decisão
**para em quem é declarado**: os *usos* daquele mesmo registro como base única
de outro (`Record::base_class`, emitido como `extends`) e os *construtores*
sintéticos dele não são revistos. O resultado é um registro que é `mixin` numa
ponta e tratado como `class` na outra.

Também é a causa dos 4 `unnecessary_type_check`: `fig.dart:26` faz
`this is AreaPosInterface ? this : null` dentro de uma classe que já aplica
`AreaPosInterface` como mixin, então o teste é sempre verdadeiro — em C++ o
`dynamic_cast` correspondente podia falhar.

**Proposta.** Uma fase explícita, entre o `Module` completo e a emissão, que
decide para **cada registro** a sua forma Dart (`class` / `mixin` /
`abstract class`) olhando *todos* os pontos de herança e instanciação do
projeto, e reescreve o IR de acordo — não só a declaração, mas todos os
consumidores:

- que decisão resolve: `class` vs. `mixin` por registro;
- que dado produz: um mapa `usr → forma`, mais a reescrita de cada
  `base_class` que aponta para um registro-mixin (vira `mixins`), e a
  supressão/relocação do construtor posicional sintético de um registro-mixin
  para uma fábrica (`static X create(...)`) que os call sites passam a usar;
- onde é consumido: `emit::dart::emit_record` deixa de derivar a forma
  sozinho e passa a lê-la do IR, o que remove por construção a chance de
  declaração e uso discordarem.

Prompt: `docs/prompts/2026-08-21-02-mixin-ou-classe-decisao-global.md`

---

### F5 — Atribuição por valor (`operator=`) chama um `assignFrom` que nunca existe

**Categoria: 1**, com uma decisão de produto embutida.

**Sintoma.** 483 chamadas a `assignFrom` (883 ocorrências textuais no pacote),
nenhuma declaração em lugar nenhum. `alignfunctor.dart:67` →
`assignFrom(_m_time, Fraction(0));`. `iomei.dart:354` →
`assignFrom(_m_currentNode, _m_currentNode.append_child(name));`. Como estão
dentro de um método, o Dart as lê como `this.assignFrom(...)` — daí
`undefined_method` e não `undefined_function`.

**Causa raiz.** `lower::cpp::dart_operator_bridge_name`
(`crates/server/src/lower/cpp.rs:6295`) mapeia `operator=` para o nome-ponte
`assignFrom`. O *call site* é lowered; a *declaração* correspondente nunca é
emitida, porque na maioria dos casos ela é o `operator=` **implícito** do C++,
que não tem cursor de definição nenhum para lowerar. Quando é explícito, F1
costuma tê-lo descartado junto com o resto dos membros out-of-line.

**Proposta.** Separar os dois casos:

- `operator=` **implícito** (cópia membro a membro gerada pelo compilador): em
  C++ `a = b` sobre um valor copia; em Dart, `a = b` liga a mesma referência.
  Onde o lado direito é um temporário recém-construído (`Fraction(0)`,
  `x.append_child(n)` — a esmagadora maioria dos casos reais aqui), atribuição
  simples é uma tradução **correta**, e é o que deve ser emitido. Onde o lado
  direito é um objeto vivo, é preciso uma cópia explícita — um método
  `copyFrom` gerado no registro, campo a campo.
- `operator=` **explícito**: emitir a declaração de `assignFrom` como método
  do registro, e o call site como `alvo.assignFrom(origem)` — não como chamada
  livre de dois argumentos.

A escolha entre "atribuição simples sempre" e "`copyFrom` sempre" muda
comportamento observável (aliasing) e é decisão de produto; o prompt a expõe.

Prompt: `docs/prompts/2026-08-21-08-atribuicao-de-valor-e-assignfrom.md`

---

### F6 — Chamadas livres de `std::`/libc emitidas verbatim, sem adaptador

**Categoria: 1** (a maioria) **+ 2** para o subconjunto que precisa de fronteira
externa declarada.

**Sintoma.** 1.201 ocorrências. Duas metades:

- **`std::` sem mapeamento**: `accid.dart:193` → `horizontalMargin = max(…)`;
  `devicecontext.dart:131` → `return make_pair(_m_baseWidth, _m_baseHeight);`;
  `adjustslursfunctor.dart:300` → `return pair(0, 0);`; mais `min` (52),
  `abs` (51), `to_string` (75), `vector` (61), `make_tuple`. Aparecem como
  `undefined_method` porque estão dentro de métodos.
- **libc/POSIX**: `undefined_function` (158) — `memset` (16), `memcpy` (12),
  `free` (6), `malloc` (5), `fclose` (7), `ftello64` (7), `__builtin_expect` (4),
  `timespec` (9) — concentrados em `zip_file.dart` (70) e `pugixml.dart` (50).
  Junto vêm `undefined_class`: `_IO_FILE` (18), `stat`, `tm`, `timeval`; e
  `undefined_identifier 'basic_string'` (286), de `std::string(...)` construído
  por sintaxe funcional.

**Causa raiz.** `lower_stdlib_method_call` cobre *métodos* de contêiner
(`vector::size` → `.length` etc.), mas não há caminho equivalente para as
**funções livres** de `<algorithm>`/`<utility>`/`<cmath>`/`<string>`, nem para
libc. O lowering cai no fallback genérico de `Call`, que aceita o nome porque
`is_plain_dart_identifier` o aprova, e o emissor o imprime literalmente.

**Proposta.** Duas frentes, na mesma família porque partilham o ponto de
extensão:

1. Uma tabela de adaptadores de função livre da stdlib, no mesmo espírito de
   `lower_stdlib_method_call`: `std::max/min` → `math.max/min`, `std::abs` →
   `.abs()`, `std::to_string` → `.toString()`, `std::make_pair` →
   `SyntaxBridgePair(...)`, `std::string(x)` → `x`.
2. Para libc/POSIX, **nada disso é `std::`** — é fronteira externa. O produto já
   tem o conceito (`docs/plans/lista-de-externos.md`, `crates/server/src/externals.rs`).
   A proposta é reconhecer o símbolo como externo na ingestão e emitir a
   fronteira nomeada correspondente, nunca uma chamada livre solta. Isso é
   categoria 2.

Prompt: `docs/prompts/2026-08-21-07-adaptadores-de-stdlib-e-libc.md`

---

### F7 — `static_cast` para baixo na hierarquia é apagado

**Categoria: 1.**

**Sintoma.** 1.523 ocorrências. `iomei.dart:348` → `WriteDoc(object);` com
`object` do tipo `VrvObject` e `WriteDoc(Doc? doc)`. A mesma linha em C++
(`src/iomei.cpp:363`) é `this->WriteDoc(vrv_cast<Doc *>(object));`.
`calcstemfunctor.dart:474` → `note = parent;` (`LayerElement` numa `VrvNote?`).

**Causa raiz.** `vrv_cast` é `#define vrv_cast static_cast` em release
(`include/vrv/vrvdef.h:65`), e `lower::cpp::is_transparent_wrapper`
(`crates/server/src/lower/cpp.rs:4542`) trata `CXXStaticCastExpr` e
`CStyleCastExpr` como **invólucros transparentes**, desembrulhando-os. O
comentário justifica isso para conversões numéricas — e ali está certo, a
comparação de tipos externo/filho vira `Expr::Convert`. Mas para um downcast de
ponteiro (`Object*` → `Doc*`) os dois lados são `Type::Nullable(Record)` com
registros *diferentes*, e o caso não é reconhecido: o operando passa adiante
carregando o tipo da base.

Repare que `dynamic_cast` **é** tratado, e bem
(`lower_dynamic_cast_expr`, `cpp.rs:5049`, emite `x is T ? x : null`) — só que
o Verovio quase nunca usa `dynamic_cast` diretamente; usa a macro.

**Proposta.** Reconhecer, em `is_transparent_wrapper`/`lower_expr`, o caso em
que um `static_cast`/cast C-style muda o registro de um ponteiro, e emitir a
mesma forma que `lower_dynamic_cast_expr` já produz para o caso checado — com a
diferença de que `static_cast` é **não checado** em C++: a tradução honesta é um
`as T?` (que estoura em tempo de execução se errado), não um `is ? : null` (que
silenciosamente vira nulo). Reaproveitar o guard de operando simples que
`lower_dynamic_cast_expr` já tem.

Prompt: `docs/prompts/2026-08-21-05-downcast-de-hierarquia-preservado.md`

---

### F8 — Parâmetro de saída por referência vira `late` local sem escrita de volta

**Categoria: 1.**

**Sintoma.** 179 erros. `adjustarpegfunctor.dart`:

```dart
46:    late int minTopLeft;
47:    late int maxTopRight;
48:    topNote!.GetAlignment()!.GetLeftRight(staffN, minTopLeft, maxTopRight);
49:    …usa minTopLeft…                       // ← definitely_unassigned
```

Em C++, `GetLeftRight(int staffN, int &minLeft, int &maxRight)` escreve nos dois.
Em Dart, `int` é passado por valor: a chamada não escreve nada, o `late` nunca
recebe valor, e o Dart detecta isso estaticamente. Os 155
`unused_local_variable` são o mesmo fenômeno visto do outro lado.

**Causa raiz.** O produto já tem a ponte certa —
`lower::cpp::apply_out_param_bridge` e `Stmt::TupleAssign` (a
`emit::dart::TUPLE_ASSIGN_TEMP` e `is_tuple_assign_discard` documentam a
rodada 20) transformam um out-param num valor de retorno em tupla. Mas ela não
está sendo aplicada a estes casos — parâmetros `T&` de tipo escalar em métodos
cuja declaração vem de um header, exatamente o conjunto que F1 também derruba.
Provável interação: a ponte precisa da declaração *e* da definição, e o merge
"primeiro vence" entrega só uma delas.

**Proposta.** Depois de F1 estar em pé, re-medir. Se persistir, estender
`apply_out_param_bridge` para cobrir os casos restantes; se o `late` local
continuar sendo a forma emitida, ele precisa ser inicializado com o valor
neutro do tipo em vez de `late`, e a chamada precisa ser a forma de tupla.
Nunca deixar `late` sem escrita — é erro de compilação, não um bailout honesto.

Prompt: `docs/prompts/2026-08-21-10-parametros-de-saida-por-referencia.md`

---

### F9 — Bailout tipado vazando: `SyntaxBridgeOpaque` duplicado e usado como valor

**Categoria: 1.**

**Sintoma.** 1.276 ocorrências. Três problemas distintos, mesma origem:

1. **`SyntaxBridgeOpaque` é redeclarado em 77 arquivos.** Cada declaração é uma
   classe diferente para o Dart. Daí os 6 `invalid_override`:
   `'void Function(int, SyntaxBridgeOpaque)' isn't a valid override of
   'void Function(int, SyntaxBridgeOpaque)'` — mesmos nomes, classes
   diferentes. `SyntaxBridgePair` e `SyntaxBridgeNativeHandle` já moram
   corretamente em `syntax_bridge_support.dart`; `SyntaxBridgeOpaque` (definido
   como `OPAQUE_TYPE_NAME` em `emit/dart.rs:26`) ficou de fora.
2. **O valor opaco é usado como se fosse do tipo real.** `argument_type_not_assignable`
   (313), `invalid_assignment` (160), `undefined_getter`/`undefined_setter`
   sobre `SyntaxBridgeOpaque`, `non_bool_condition`/`non_bool_negation_expression`
   quando um bailout cai numa condição (`chord.dart:283`),
   `constant_pattern_never_matches_value_type` (21) quando cai num `switch`.
   O `AGENTS.md` exige que "bailouts de expressão preservem o tipo estático
   esperado e falhem explicitamente" — `Expr::UnsupportedTyped` existe para
   isso, mas há caminhos emitindo `SyntaxBridgeOpaque` cru.
3. **`_syntaxBridgeUnsupported<…>(…)` em posição de padrão.** `iomei.dart:3271`
   e `jsonxx.dart` produzem `not_a_type` +
   `positional_field_in_object_pattern` + `refutable_pattern_in_irrefutable_context`
   na mesma linha: um bailout de expressão foi colocado onde o Dart espera um
   *tipo*, o que nem sintaticamente funciona.

**Proposta.**

- Mover `SyntaxBridgeOpaque` para `syntax_bridge_support.dart` e importá-lo —
  correção mecânica, resolve os `invalid_override` e a incompatibilidade entre
  arquivos por construção.
- Auditar os caminhos que ainda produzem `Expr::Unsupported` sem tipo em
  posição de valor e convertê-los para `UnsupportedTyped` com o tipo estático
  esperado do contexto, como o `AGENTS.md` exige.
- Em posição de *padrão* (desestruturação, `case`, `on ... catch`), um bailout
  de expressão não é representável: precisa virar bailout de **statement**
  (`Stmt::Unsupported`) que substitui o bloco inteiro, não um buraco no meio da
  sintaxe.

Prompt: `docs/prompts/2026-08-21-06-bailout-tipado-e-opaque-compartilhado.md`

---

### F10 — Iteradores STL vazam como tipo

**Categoria: 1**, mas com alcance maior do que a contagem sugere.

**Sintoma.** 44 diagnósticos diretos (`undefined_class '__normal_iterator'` 28,
`non_type_as_type_argument`, `for_in_of_invalid_type 'xpath_node_set'` 8), mas
o padrão em `alignfunctor.dart:662` mostra o alcance real:

```dart
__normal_iterator verseIterator = __normal_iterator(find_if(
  _syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…: unsupported std::vector::begin call'),
  _syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…: unsupported std::vector::end call'),
  ObjectComparison(ClassId.VERSE)));
```

Um único idioma C++ (`std::find_if(v.begin(), v.end(), pred)`) produz quatro
falhas encadeadas: dois bailouts de `begin`/`end`, um `find_if` sem adaptador
(F6), e um tipo `__normal_iterator` inexistente. O `.unsupportedOperator()`
logo depois é a desreferência do iterador.

**Proposta.** Tratar o **idioma inteiro**, não o iterador isolado: reconhecer
`begin`/`end` de um contêiner como delimitadores de uma travessia e mapear os
algoritmos que os consomem para o equivalente Dart (`find_if` →
`.firstWhere(..., orElse: () => null)`, `sort` → `.sort(...)`, etc.).
`__normal_iterator` como tipo nomeado só deve sobreviver quando o iterador for
guardado numa variável de vida longa — e aí a resposta é um adaptador nomeado
(um cursor sobre `List<T>`), não o nome interno da libstdc++.

Prompt: `docs/prompts/2026-08-21-13-iteradores-stl.md`

---

### F11 — Aritmética `int`/`double`

**Categoria: 1.**

**Sintoma.** 153 ocorrências, 135 delas `A value of type 'double' can't be
assigned to a variable of type 'int'`. `accid.dart:155`:

```dart
int horizontalMargin = doc!.GetOptionsConst()!.m_ledgerLineExtension.GetValue()
    * unit.toDouble() + 0.5 * rightMargin.toDouble().toInt();
```

O `.toInt()` foi colocado no operando (`rightMargin.toDouble().toInt()`, que é
um no-op caro) em vez de no resultado da expressão inteira.

**Causa raiz.** Em C++ `int x = a * 0.5 + b;` narrowing implícito é legal e a
conversão acontece **na atribuição**. `Expr::Convert` está sendo inserido por
operando, e não na fronteira certa.

**Proposta.** Aplicar a conversão na fronteira de atribuição/retorno/argumento
— o ponto onde o C++ também a aplica — em vez de por operando, e remover
`.toDouble().toInt()` encadeados que se anulam.

Prompt: `docs/prompts/2026-08-21-11-aritmetica-int-double.md`

---

### F12 — Chamada qualificada à base vira recursão infinita

**Categoria: 1.** *Esta família não aparece no `dart analyze`* — foi encontrada
lendo o Dart emitido, e é a mais grave do documento em termos de correção.

**Sintoma.** `abbr.dart`:

```dart
@override
void Reset() {
  Reset();          // ← recursão infinita
  ResetSource();
}
```

O C++ (`src/abbr.cpp:35-38`) é:

```cpp
void Abbr::Reset()
{
    EditorialElement::Reset();
    this->ResetSource();
}
```

A chamada qualificada à base perdeu a qualificação. `super.` **não aparece uma
única vez** nos 301 arquivos emitidos. Uma varredura conservadora (só o
primeiro statement do método, só assinaturas de uma linha) acha 61 métodos
auto-recursivos; o número real é maior, e vai crescer muito quando F1 trouxer
de volta os métodos out-of-line — que é onde a maioria dos overrides do Verovio
mora.

O `dart analyze` não reporta nada: é Dart perfeitamente válido que estoura a
pilha em tempo de execução. Exatamente o tipo de silêncio que o `AGENTS.md`
proíbe.

**Proposta.** Lowerar `Base::metodo(args)` dentro de um método de uma
subclasse como `super.metodo(args)`. Com a cadeia de mixins achatada que F4
produz, `super` resolve pela linearização do Dart, que **não** é necessariamente
a base que o C++ nomeou — quando a base nomeada não for a imediatamente
anterior na linearização, isso precisa ser um bailout explícito, não um
`super.` que chama outra coisa.

Prompt: `docs/prompts/2026-08-21-09-chamada-a-base-qualificada.md`

---

### F13 — Overloads que colidem no mesmo nome Dart

**Categoria: 1.**

**Sintoma.** 118 `duplicate_definition`. `adjustslursfunctor.dart:641` →
`The name 'CalcEndPointShift' is already defined`; idem `IsInsideArtic`
(`artic.dart`), `GetRectangles` (`boundingbox.dart`), `is_` (`jsonxx.dart`,
11), `write` (6), `streamInsert` (36, em `humlib.dart`).

**Causa raiz.** Dois motivos distintos:

- **Par `const`/não-`const`**: `int GetX()` e `int GetX() const` são dois
  membros diferentes em C++ e o mesmo nome em Dart. `function_catalog::apply_overload_renames`
  já renomeia overloads por assinatura, mas `const`-ness não faz parte da
  assinatura que ele considera. (Note que o pipeline *já* trata isso em alguns
  casos, gerando pares `GetOptions`/`GetOptionsConst` — logo é inconsistência,
  não ausência total.)
- **Operadores-ponte**: todos os `operator<<` de um arquivo viram
  `streamInsert`, sem sufixo de aridade/tipo.

**Proposta.** Incluir `const`-ness e o tipo dos parâmetros na chave de
desambiguação de `apply_overload_renames`, e aplicar a mesma desambiguação aos
nomes-ponte de `dart_operator_bridge_name`.

Prompt: `docs/prompts/2026-08-21-12-overloads-const-e-colisoes-de-nome.md`

---

### F14 — Parentização e precedência na emissão

**Categoria: 1.**

**Sintoma.** Poucas ocorrências (~55), mas cada uma é código silenciosamente
errado, não só um erro do analisador:

```dart
iohumdrum.dart:9035:  if (solo ? 1 : 0 == true.toInt()) {
```

O C++ é `if (solo == true)` com `solo` sendo `int`; a conversão
`int`→`bool` virou um ternário sem parênteses, que o Dart lê como
`solo ? 1 : (0 == …)`.

```dart
editortoolkit_neume.dart:1835:  oldClef = sparent is Layer ? sparent : null!.GetCurrentClef();
```

Deveria ser `(sparent is Layer ? sparent : null)!.GetCurrentClef()` — a forma
emitida chama um método em cima de `null` literal (`null_check_always_fails` +
`receiver_of_type_never`).

**Proposta.** Toda expressão composta emitida em posição de operando,
receptor ou condição precisa ser parentizada pelo emissor, em vez de depender
da precedência coincidir entre C++ e Dart. É barato e elimina uma classe
inteira de erro silencioso.

Prompt: `docs/prompts/2026-08-21-14-parentizacao-e-precedencia.md`

---

### F15 — Resíduos de emissão (limpeza)

**Categoria: 1**, itens independentes e pequenos, agrupados num prompt só
porque cada um é uma correção local de poucas linhas:

| Item | `code` | n | O que fazer |
| --- | --- | ---: | --- |
| `break` depois de `return` num `case` | `dead_code` | 83 | Suprimir o terminador redundante (`accid.dart:242`) |
| `import` não usado | `unused_import` | 143 | Emitir só os imports realmente referenciados |
| `SyntaxBridgePair.first/second` são `final` | `assignment_to_final` | 39 | `std::pair` é mutável em C++ (`v.second = i`) — os campos precisam ser mutáveis |
| Default de parâmetro não-const | `const_eval_method_invocation` + `const_eval_property_access` | 15 | `0.toDouble()` → `0.0`; `PenStyle.PEN_SOLID.value` → a constante literal |
| `for` sobre `String`/`Map` | `for_in_of_invalid_type` | ~16 | `for (c in s)` → `s.codeUnits`; sobre `Map` → `.entries`/`.values` |
| Chave de mapa com enum onde o tipo é `int` | `map_key_type_not_assignable` | 6 | Usar `.value` na chave, ou tipar o mapa pelo enum |
| Variádicos (`LogError('%s', str)`) | `extra_positional_arguments` | parte dos 481 | C++ `...` não tem equivalente posicional: fronteira explícita |
| Local que esconde o nome do tipo (`tm tm = tm(…)`) | `referenced_before_declaration` | 3 | Renomear a local no lowering |
| `main` com assinatura C | `main_first_positional_parameter_type` | 1 | `main(List<String> args)` |

Prompt: `docs/prompts/2026-08-21-15-limpeza-de-emissao.md`

## 5. Ordem sugerida de execução

Ordenada por alavancagem — quanto cada correção destrava, não só quanto o
`code` isolado conta. F1 vem primeiro porque metade do relatório depende dela e
porque várias outras famílias só podem ser **medidas** depois que ela estiver em
pé (F8 e boa parte de `unused_field`, em particular).

| # | Prompt | Família | Ocorrências alvo |
| ---: | --- | --- | ---: |
| 01 | `2026-08-21-01-uniao-de-registros-no-merge.md` | F1 | ~9.500 (+ parte de 1.759) |
| 02 | `2026-08-21-02-mixin-ou-classe-decisao-global.md` | F4 | ~845 (estrutural) |
| 03 | `2026-08-21-03-membros-privados-entre-bibliotecas.md` | F2 | ~1.170 (+ parte de 1.759) |
| 04 | `2026-08-21-04-bang-redundante-e-promocao.md` | F3 | 6.107 |
| 05 | `2026-08-21-05-downcast-de-hierarquia-preservado.md` | F7 | ~1.520 |
| 06 | `2026-08-21-06-bailout-tipado-e-opaque-compartilhado.md` | F9 | ~1.280 |
| 07 | `2026-08-21-07-adaptadores-de-stdlib-e-libc.md` | F6 | ~1.200 |
| 08 | `2026-08-21-08-atribuicao-de-valor-e-assignfrom.md` | F5 | 483 |
| 09 | `2026-08-21-09-chamada-a-base-qualificada.md` | F12 | 0 no analyze, ≥61 bugs reais |
| 10 | `2026-08-21-10-parametros-de-saida-por-referencia.md` | F8 | 179 (+155 avisos) |
| 11 | `2026-08-21-11-aritmetica-int-double.md` | F11 | 153 |
| 12 | `2026-08-21-12-overloads-const-e-colisoes-de-nome.md` | F13 | 118 |
| 13 | `2026-08-21-13-iteradores-stl.md` | F10 | 44 (+ destrava bailouts) |
| 14 | `2026-08-21-14-parentizacao-e-precedencia.md` | F14 | ~55 (erro silencioso) |
| 15 | `2026-08-21-15-limpeza-de-emissao.md` | F15 | ~450 |

Depois de **cada** prompt, rodar `just verovio-diagnosis` de novo e comparar o
agrupamento por `code`: os alvos devem cair, e nenhum outro grupo deve subir sem
justificativa registrada. As contagens-alvo acima são estimativas da atribuição
heurística de §2, não promessas.

## Anexo A — arquivos por `code`

Lista completa: para cada `code`, todos os arquivos onde ele ocorre, com a
contagem por arquivo, em ordem decrescente. Caminhos relativos a `lib/`.

**`undefined_method`** — 8309 ocorrências · 145 arquivos · ERROR

iohumdrum.dart (2456), iomei.dart (1952), editortoolkit_neume.dart (463), svgdevicecontext.dart (265), midifunctor.dart (218), convertfunctor.dart (202), iopae.dart (172), boundingbox.dart (151), humlib.dart (135), alignfunctor.dart (123), iocmme.dart (118), adjustslursfunctor.dart (114), ioabc.dart (111), pugixml.dart (93), editortoolkit_shared.dart (90), calcstemfunctor.dart (88), adjustfloatingpositionerfunctor.dart (73), beam.dart (73), castofffunctor.dart (61), atts_header.dart (55), adjustxposfunctor.dart (44), accid.dart (41), adjusttupletsyfunctor.dart (40), chord.dart (38), facsimilefunctor.dart (34), artic.dart (32), adjustgracexposfunctor.dart (29), calcchordnoteheadsfunctor.dart (29), bboxdevicecontext.dart (27), featureextractor.dart (26), hairpin.dart (26), adjustbeamsfunctor.dart (25), devicecontextbase.dart (25), barline.dart (24), findfunctor.dart (23), findlayerelementsfunctor.dart (23), devicecontext.dart (22), dynam.dart (22), clef.dart (21), iogabc.dart (21), jsonxx.dart (21), justifyfunctor.dart (21), adjustarticfunctor.dart (20), adjustharmgrpsspacingfunctor.dart (20), atts_pagebased.dart (20), btrem.dart (19), tunings.dart (19), calcbboxoverflowsfunctor.dart (18), calcdotsfunctor.dart (17), att.dart (16), calcslurdirectionfunctor.dart (16), custos.dart (16), keyaccid.dart (16), adjustclefchangesfunctor.dart (14), arpeg.dart (12), beatrpt.dart (12), caesura.dart (12), controlelement.dart (11), dir.dart (11), adjustarpegfunctor.dart (10), atts_edittrans.dart (10), atts_figtable.dart (10), bracketspan.dart (10), editortoolkit.dart (10), graphic.dart (10), adjustxoverflowfunctor.dart (9), adjustyposfunctor.dart (9), annotscore.dart (9), beamspan.dart (9), calcarticfunctor.dart (9), calcledgerlinesfunctor.dart (9), calcligatureorneumeposfunctor.dart (9), comparison.dart (9), object.dart (9), adjustossiastaffdeffunctor.dart (8), adjusttupletsxfunctor.dart (8), course.dart (8), div.dart (8), gliss.dart (8), instrdef.dart (8), cpmark.dart (7), divline.dart (7), drawinginterface.dart (7), f.dart (7), fing.dart (7), layerdef.dart (7), adjustaccidxfunctor.dart (6), adjusttempofunctor.dart (6), anchoredtext.dart (6), binasc.dart (6), breath.dart (6), calcalignmentxposfunctor.dart (6), editfunctor.dart (6), halfmrpt.dart (6), lv.dart (6), adjustlayersfunctor.dart (5), adjustneumexfunctor.dart (5), adjustsylspacingfunctor.dart (5), atts_fingering.dart (5), atts_harmony.dart (5), atts_performance.dart (5), cachehorizontallayoutfunctor.dart (5), episema.dart (5), fb.dart (5), genericlayerelement.dart (5), zip_file.dart (5), adjustdotsfunctor.dart (4), lb.dart (4), liquescent.dart (4), midimessage.dart (4), altsyminterface.dart (3), annot.dart (3), calcalignmentpitchposfunctor.dart (3), customtuning.dart (3), midieventlist.dart (3), midifile.dart (3), staff.dart (3), areaposinterface.dart (2), horizontalaligner.dart (2), layerelement.dart (2), note.dart (2), resources.dart (2), savefunctor.dart (2), slur.dart (2), toolkit.dart (2), abbr.dart (1), add.dart (1), adjuststaffoverlapfunctor.dart (1), adjustxrelfortranscriptionfunctor.dart (1), adjustyrelfortranscriptionfunctor.dart (1), corr.dart (1), damage.dart (1), del.dart (1), doc.dart (1), facsimileinterface.dart (1), keysig.dart (1), layer.dart (1), measure.dart (1), mensur.dart (1), metersig.dart (1), metersiggrp.dart (1), midievent.dart (1), page.dart (1), svg.dart (1), system.dart (1)

**`unnecessary_non_null_assertion`** — 6107 ocorrências · 77 arquivos · WARNING

iomei.dart (1196), iohumdrum.dart (1082), zip_file.dart (577), editortoolkit_neume.dart (541), alignfunctor.dart (182), pugixml.dart (179), beam.dart (169), convertfunctor.dart (163), calcstemfunctor.dart (160), midifunctor.dart (152), iopae.dart (138), iocmme.dart (118), svgdevicecontext.dart (108), adjusttupletsyfunctor.dart (84), adjustfloatingpositionerfunctor.dart (79), adjustxposfunctor.dart (68), ioabc.dart (61), findfunctor.dart (60), boundingbox.dart (59), calcligatureorneumeposfunctor.dart (54), adjustarticfunctor.dart (47), facsimilefunctor.dart (46), calcchordnoteheadsfunctor.dart (44), calcslurdirectionfunctor.dart (43), calcbboxoverflowsfunctor.dart (41), adjusttupletsxfunctor.dart (40), findlayerelementsfunctor.dart (40), adjustslursfunctor.dart (39), justifyfunctor.dart (36), adjustbeamsfunctor.dart (35), iogabc.dart (33), accid.dart (30), calcarticfunctor.dart (28), calcdotsfunctor.dart (27), adjustclefchangesfunctor.dart (25), devicecontext.dart (22), barline.dart (20), castofffunctor.dart (20), calcledgerlinesfunctor.dart (17), editortoolkit_shared.dart (17), hairpin.dart (17), adjustarpegfunctor.dart (15), adjustharmgrpsspacingfunctor.dart (15), featureextractor.dart (14), adjustgracexposfunctor.dart (13), adjustyposfunctor.dart (11), editfunctor.dart (11), adjustossiastaffdeffunctor.dart (10), comparison.dart (10), controlelement.dart (9), adjustlayersfunctor.dart (8), adjustneumexfunctor.dart (7), bboxdevicecontext.dart (7), btrem.dart (7), adjusttempofunctor.dart (6), artic.dart (6), adjustxoverflowfunctor.dart (5), cachehorizontallayoutfunctor.dart (5), chord.dart (5), adjustdotsfunctor.dart (4), adjustxrelfortranscriptionfunctor.dart (4), adjustyrelfortranscriptionfunctor.dart (4), editortoolkit.dart (4), adjustaccidxfunctor.dart (3), caesura.dart (3), calcalignmentxposfunctor.dart (3), custos.dart (3), devicecontextbase.dart (3), fing.dart (3), calcalignmentpitchposfunctor.dart (2), clef.dart (2), jsonxx.dart (2), savefunctor.dart (2), altsyminterface.dart (1), arpeg.dart (1), beamspan.dart (1), humlib.dart (1)

**`unused_field`** — 1759 ocorrências · 67 arquivos · WARNING

humlib.dart (1362), iomusxml.dart (50), pugixml.dart (34), setscoredeffunctor.dart (34), preparedatafunctor.dart (27), iohumdrum.dart (24), doc.dart (21), midifile.dart (12), svgdevicecontext.dart (9), toolkit.dart (9), ioabc.dart (8), options.dart (8), devicecontext.dart (7), object.dart (7), resources.dart (7), scoringupfunctor.dart (7), adjustxposfunctor.dart (6), glyph.dart (6), transposefunctor.dart (6), view.dart (6), bboxdevicecontext.dart (5), convertfunctor.dart (5), durationinterface.dart (5), midifunctor.dart (5), verticalaligner.dart (5), alignfunctor.dart (4), calcalignmentxposfunctor.dart (4), iomei.dart (4), jsonxx.dart (4), pages.dart (4), transposition.dart (4), tunings.dart (4), calcdotsfunctor.dart (3), floatingobject.dart (3), adjustfloatingpositionerfunctor.dart (2), adjustgracexposfunctor.dart (2), adjustsylspacingfunctor.dart (2), boundingbox.dart (2), calcalignmentpitchposfunctor.dart (2), castofffunctor.dart (2), comparison.dart (2), drawinginterface.dart (2), findlayerelementsfunctor.dart (2), iobase.dart (2), iogabc.dart (2), layerelement.dart (2), linkinginterface.dart (2), miscfunctor.dart (2), textlayoutelement.dart (2), timeinterface.dart (2), zip_file.dart (2), adjustaccidxfunctor.dart (1), adjustbeamsfunctor.dart (1), adjuststaffoverlapfunctor.dart (1), binasc.dart (1), editortoolkit.dart (1), facsimilefunctor.dart (1), filereader.dart (1), horizontalaligner.dart (1), iopae.dart (1), justifyfunctor.dart (1), keysig.dart (1), midievent.dart (1), midieventlist.dart (1), page.dart (1), plistinterface.dart (1), symboldef.dart (1)

**`argument_type_not_assignable`** — 1569 ocorrências · 31 arquivos · ERROR

iohumdrum.dart (577), editortoolkit_neume.dart (233), iomei.dart (230), zip_file.dart (164), pugixml.dart (150), featureextractor.dart (44), iopae.dart (39), ioabc.dart (30), boundingbox.dart (20), humlib.dart (16), jsonxx.dart (9), tuningsimpl.dart (8), adjustslursfunctor.dart (6), editortoolkit_shared.dart (6), vrv.dart (6), adjustfloatingpositionerfunctor.dart (4), binasc.dart (4), convertfunctor.dart (4), iogabc.dart (3), adjustarticfunctor.dart (2), castofffunctor.dart (2), savefunctor.dart (2), svgdevicecontext.dart (2), adjustdotsfunctor.dart (1), adjusttupletsyfunctor.dart (1), chord.dart (1), devicecontextbase.dart (1), findfunctor.dart (1), iocmme.dart (1), midifunctor.dart (1), tunings.dart (1)

**`undefined_identifier`** — 1223 ocorrências · 52 arquivos · ERROR

editortoolkit_neume.dart (385), iohumdrum.dart (272), iomei.dart (103), pugixml.dart (86), iopae.dart (42), jsonxx.dart (25), editortoolkit_shared.dart (23), alignfunctor.dart (21), vrv.dart (20), adjustfloatingpositionerfunctor.dart (19), calcstemfunctor.dart (18), ioabc.dart (15), iocmme.dart (15), adjustxposfunctor.dart (13), adjustbeamsfunctor.dart (11), adjustarticfunctor.dart (10), calcchordnoteheadsfunctor.dart (10), humlib.dart (10), castofffunctor.dart (9), iogabc.dart (9), adjustslursfunctor.dart (8), calcligatureorneumeposfunctor.dart (8), tunings.dart (8), adjustgracexposfunctor.dart (7), calcledgerlinesfunctor.dart (7), adjusttupletsyfunctor.dart (6), bboxdevicecontext.dart (6), calcdotsfunctor.dart (6), adjustyposfunctor.dart (5), dynam.dart (5), zip_file.dart (5), adjustclefchangesfunctor.dart (4), justifyfunctor.dart (4), calcarticfunctor.dart (3), adjustharmgrpsspacingfunctor.dart (2), adjustneumexfunctor.dart (2), barline.dart (2), binasc.dart (2), calcalignmentxposfunctor.dart (2), calcbboxoverflowsfunctor.dart (2), tuningsimpl.dart (2), adjustaccidxfunctor.dart (1), adjustarpegfunctor.dart (1), adjustlayersfunctor.dart (1), adjustossiastaffdeffunctor.dart (1), adjustsylspacingfunctor.dart (1), adjusttupletsxfunctor.dart (1), calcslurdirectionfunctor.dart (1), convertfunctor.dart (1), crc.dart (1), midifunctor.dart (1), svgdevicecontext.dart (1)

**`undefined_getter`** — 920 ocorrências · 32 arquivos · ERROR

iomei.dart (372), atts_shared.dart (161), iohumdrum.dart (73), editortoolkit_neume.dart (49), atts_visual.dart (40), iocmme.dart (30), zip_file.dart (29), atts_pagebased.dart (26), svgdevicecontext.dart (17), atts_header.dart (14), editortoolkit_shared.dart (13), atts_cmn.dart (12), atts_midi.dart (12), convertfunctor.dart (10), findlayerelementsfunctor.dart (8), pugixml.dart (8), midifunctor.dart (7), jsonxx.dart (5), atts_edittrans.dart (4), beatrpt.dart (4), justifyfunctor.dart (4), measure.dart (4), alignfunctor.dart (2), atts_figtable.dart (2), atts_fingering.dart (2), atts_harmony.dart (2), atts_performance.dart (2), horizontalaligner.dart (2), iomusxml.dart (2), scoredef.dart (2), adjusttupletsyfunctor.dart (1), bboxdevicecontext.dart (1)

**`override_on_non_overriding_member`** — 750 ocorrências · 194 arquivos · WARNING

convertfunctor.dart (63), castofffunctor.dart (49), midifunctor.dart (48), alignfunctor.dart (35), savefunctor.dart (23), facsimilefunctor.dart (18), preparedatafunctor.dart (18), justifyfunctor.dart (12), calcstemfunctor.dart (10), div.dart (10), setscoredeffunctor.dart (9), adjustbeamsfunctor.dart (8), adjustfloatingpositionerfunctor.dart (8), chord.dart (8), fb.dart (8), lb.dart (8), accid.dart (7), adjustgracexposfunctor.dart (7), adjustyposfunctor.dart (7), beam.dart (7), btrem.dart (7), course.dart (7), findlayerelementsfunctor.dart (7), genericlayerelement.dart (7), layerdef.dart (7), adjustarticfunctor.dart (6), adjustlayersfunctor.dart (6), adjustneumexfunctor.dart (6), adjustossiastaffdeffunctor.dart (6), adjustsylspacingfunctor.dart (6), adjustxposfunctor.dart (6), custos.dart (6), editfunctor.dart (6), f.dart (6), graphic.dart (6), instrdef.dart (6), adjustdotsfunctor.dart (5), adjustharmgrpsspacingfunctor.dart (5), adjusttupletsyfunctor.dart (5), adjustxoverflowfunctor.dart (5), artic.dart (5), barline.dart (5), beatrpt.dart (5), calcalignmentpitchposfunctor.dart (5), calcalignmentxposfunctor.dart (5), calcchordnoteheadsfunctor.dart (5), clef.dart (5), halfmrpt.dart (5), keyaccid.dart (5), adjustaccidxfunctor.dart (4), adjustarpegfunctor.dart (4), app.dart (4), cachehorizontallayoutfunctor.dart (4), calcarticfunctor.dart (4), calcdotsfunctor.dart (4), calcledgerlinesfunctor.dart (4), choice.dart (4), controlelement.dart (4), elementpart.dart (4), findfunctor.dart (4), adjustclefchangesfunctor.dart (3), adjustslursfunctor.dart (3), adjuststaffoverlapfunctor.dart (3), adjusttempofunctor.dart (3), calcbboxoverflowsfunctor.dart (3), calcligatureorneumeposfunctor.dart (3), calcslurdirectionfunctor.dart (3), resetfunctor.dart (3), abbr.dart (2), add.dart (2), adjusttupletsxfunctor.dart (2), adjustxrelfortranscriptionfunctor.dart (2), adjustyrelfortranscriptionfunctor.dart (2), anchoredtext.dart (2), annot.dart (2), annotscore.dart (2), beamspan.dart (2), calcspanningbeamspansfunctor.dart (2), corr.dart (2), cpmark.dart (2), damage.dart (2), del.dart (2), dir.dart (2), dynam.dart (2), editortoolkit_shared.dart (2), fing.dart (2), horizontalaligner.dart (2), miscfunctor.dart (2), mspace.dart (2), num.dart (2), ref.dart (2), stem.dart (2), subst.dart (2), symboltable.dart (2), text.dart (2), arpeg.dart (1), bracketspan.dart (1), breath.dart (1), caesura.dart (1), divline.dart (1), doc.dart (1), dot.dart (1), ending.dart (1), episema.dart (1), expan.dart (1), expansion.dart (1), facsimile.dart (1), fermata.dart (1), fig.dart (1), floatingobject.dart (1), ftrem.dart (1), gliss.dart (1), gracegrp.dart (1), grpsym.dart (1), hairpin.dart (1), harm.dart (1), keysig.dart (1), label.dart (1), labelabbr.dart (1), layer.dart (1), lem.dart (1), ligature.dart (1), liquescent.dart (1), lv.dart (1), mdiv.dart (1), measure.dart (1), mensur.dart (1), metersig.dart (1), metersiggrp.dart (1), mnum.dart (1), mordent.dart (1), mrest.dart (1), mrpt.dart (1), mrpt2.dart (1), multirest.dart (1), multirpt.dart (1), nc.dart (1), neume.dart (1), note.dart (1), octave.dart (1), orig.dart (1), oriscus.dart (1), ornam.dart (1), ossia.dart (1), page.dart (1), pagemilestone.dart (1), pb.dart (1), pedal.dart (1), pitchinflection.dart (1), plica.dart (1), proport.dart (1), quilisma.dart (1), rdg.dart (1), reg.dart (1), reh.dart (1), rend.dart (1), repeatmark.dart (1), rest.dart (1), restore.dart (1), runningelement.dart (1), sb.dart (1), scoredef.dart (1), scoringupfunctor.dart (1), section.dart (1), sic.dart (1), slur.dart (1), space.dart (1), staff.dart (1), staffdef.dart (1), staffgrp.dart (1), strophicus.dart (1), supplied.dart (1), surface.dart (1), svg.dart (1), syl.dart (1), syllable.dart (1), symbol.dart (1), symboldef.dart (1), systemmilestone.dart (1), tabdursym.dart (1), tabgrp.dart (1), tempo.dart (1), tie.dart (1), timestamp.dart (1), transposefunctor.dart (1), trill.dart (1), tuning.dart (1), tunings.dart (1), tuplet.dart (1), turn.dart (1), unclear.dart (1), verse.dart (1), verticalaligner.dart (1), zone.dart (1)

**`not_enough_positional_arguments`** — 583 ocorrências · 16 arquivos · ERROR

editortoolkit_neume.dart (245), iohumdrum.dart (120), iomei.dart (52), pugixml.dart (46), iocmme.dart (20), featureextractor.dart (18), castofffunctor.dart (16), ioabc.dart (15), convertfunctor.dart (13), iopae.dart (10), iogabc.dart (9), editortoolkit_shared.dart (7), zip_file.dart (6), boundingbox.dart (2), jsonxx.dart (2), midifunctor.dart (2)

**`invalid_assignment`** — 564 ocorrências · 61 arquivos · ERROR

editortoolkit_neume.dart (70), zip_file.dart (58), pugixml.dart (36), boundingbox.dart (35), beam.dart (27), svgdevicecontext.dart (27), adjustslursfunctor.dart (26), iomei.dart (24), iopae.dart (22), jsonxx.dart (18), midifunctor.dart (18), iocmme.dart (16), alignfunctor.dart (14), iohumdrum.dart (13), castofffunctor.dart (11), comparison.dart (11), adjusttupletsyfunctor.dart (10), adjustxposfunctor.dart (10), convertfunctor.dart (10), calcstemfunctor.dart (9), barline.dart (7), vrv.dart (7), adjustbeamsfunctor.dart (6), hairpin.dart (6), accid.dart (5), calcbboxoverflowsfunctor.dart (5), chord.dart (5), adjustclefchangesfunctor.dart (4), adjustfloatingpositionerfunctor.dart (4), devicecontextbase.dart (4), editortoolkit_shared.dart (4), adjustgracexposfunctor.dart (3), adjustharmgrpsspacingfunctor.dart (2), adjustneumexfunctor.dart (2), adjustxoverflowfunctor.dart (2), bboxdevicecontext.dart (2), calcchordnoteheadsfunctor.dart (2), calcligatureorneumeposfunctor.dart (2), calcslurdirectionfunctor.dart (2), featureextractor.dart (2), findfunctor.dart (2), justifyfunctor.dart (2), adjustaccidxfunctor.dart (1), adjustarpegfunctor.dart (1), adjusttempofunctor.dart (1), adjustyposfunctor.dart (1), altsyminterface.dart (1), artic.dart (1), attalternates.dart (1), bracketspan.dart (1), clef.dart (1), controlelement.dart (1), devicecontext.dart (1), dynam.dart (1), editfunctor.dart (1), facsimilefunctor.dart (1), findlayerelementsfunctor.dart (1), fing.dart (1), glyph.dart (1), ioabc.dart (1), meibasic.dart (1)

**`extra_positional_arguments`** — 481 ocorrências · 65 arquivos · ERROR

iomei.dart (75), atts_shared.dart (51), editortoolkit_neume.dart (43), svgdevicecontext.dart (31), boundingbox.dart (30), humlib.dart (28), iohumdrum.dart (26), adjustslursfunctor.dart (24), atts_visual.dart (12), ioabc.dart (12), iopae.dart (11), convertfunctor.dart (10), atts_pagebased.dart (8), castofffunctor.dart (6), beam.dart (5), iocmme.dart (5), iogabc.dart (5), pugixml.dart (5), atts_cmn.dart (4), atts_midi.dart (4), bboxdevicecontext.dart (4), calcstemfunctor.dart (4), findlayerelementsfunctor.dart (4), iomusxml.dart (4), justifyfunctor.dart (4), vrv.dart (4), adjusttupletsxfunctor.dart (3), beatrpt.dart (3), customtuning.dart (3), editortoolkit.dart (3), editortoolkit_shared.dart (3), featureextractor.dart (3), midifunctor.dart (3), arpeg.dart (2), artic.dart (2), attalternates.dart (2), calcarticfunctor.dart (2), calcslurdirectionfunctor.dart (2), chord.dart (2), facsimilefunctor.dart (2), horizontalaligner.dart (2), measure.dart (2), accid.dart (1), adjustbeamsfunctor.dart (1), adjustdotsfunctor.dart (1), adjustgracexposfunctor.dart (1), adjustharmgrpsspacingfunctor.dart (1), adjustlayersfunctor.dart (1), adjusttupletsyfunctor.dart (1), adjustxoverflowfunctor.dart (1), adjustxposfunctor.dart (1), alignfunctor.dart (1), altsyminterface.dart (1), app.dart (1), barline.dart (1), beamspan.dart (1), choice.dart (1), controlelement.dart (1), dynam.dart (1), genericlayerelement.dart (1), hairpin.dart (1), liquescent.dart (1), scoredef.dart (1), tunings.dart (1), tuningsimpl.dart (1)

**`extends_non_class`** — 361 ocorrências · 65 arquivos · ERROR

atts_shared.dart (149), atts_visual.dart (39), atts_cmn.dart (29), preparedatafunctor.dart (15), atts_gestural.dart (11), atts_analytical.dart (9), atts_midi.dart (9), atts_header.dart (7), atts_mensural.dart (6), setscoredeffunctor.dart (5), atts_cmnornaments.dart (4), atts_neumes.dart (4), atts_stringtab.dart (4), atts_usersymbols.dart (4), convertfunctor.dart (4), horizontalaligner.dart (3), midifunctor.dart (3), resetfunctor.dart (3), atts_edittrans.dart (2), atts_externalsymbols.dart (2), editfunctor.dart (2), facsimilefunctor.dart (2), miscfunctor.dart (2), verticalaligner.dart (2), accid.dart (1), adjustxoverflowfunctor.dart (1), adjustxrelfortranscriptionfunctor.dart (1), adjustyrelfortranscriptionfunctor.dart (1), app.dart (1), attconverter.dart (1), atts_critapp.dart (1), atts_facsimile.dart (1), atts_figtable.dart (1), atts_fingering.dart (1), atts_harmony.dart (1), atts_mei.dart (1), atts_pagebased.dart (1), atts_performance.dart (1), castofffunctor.dart (1), choice.dart (1), div.dart (1), doc.dart (1), elementpart.dart (1), fb.dart (1), findfunctor.dart (1), floatingobject.dart (1), functor.dart (1), genericlayerelement.dart (1), lb.dart (1), mspace.dart (1), num.dart (1), page.dart (1), pagemilestone.dart (1), pugixml.dart (1), ref.dart (1), savefunctor.dart (1), scoringupfunctor.dart (1), subst.dart (1), svg.dart (1), symboldef.dart (1), symboltable.dart (1), systemmilestone.dart (1), text.dart (1), timestamp.dart (1), tunings.dart (1)

**`unchecked_use_of_nullable_value`** — 334 ocorrências · 4 arquivos · ERROR

pugixml.dart (248), zip_file.dart (84), editortoolkit_neume.dart (1), iohumdrum.dart (1)

**`implicit_super_initializer_missing_arguments`** — 319 ocorrências · 71 arquivos · ERROR

humlib.dart (104), atts_shared.dart (57), comparison.dart (21), atts_visual.dart (18), atts_cmn.dart (10), atts_analytical.dart (6), atts_gestural.dart (6), pugixml.dart (5), alignfunctor.dart (4), castofffunctor.dart (4), convertfunctor.dart (4), setscoredeffunctor.dart (4), adjustfloatingpositionerfunctor.dart (3), atts_usersymbols.dart (3), justifyfunctor.dart (3), preparedatafunctor.dart (3), transposefunctor.dart (3), adjustarticfunctor.dart (2), adjusttupletsyfunctor.dart (2), adjustyposfunctor.dart (2), atts_mensural.dart (2), atts_midi.dart (2), atts_neumes.dart (2), atts_stringtab.dart (2), adjustaccidxfunctor.dart (1), adjustarpegfunctor.dart (1), adjustbeamsfunctor.dart (1), adjustclefchangesfunctor.dart (1), adjustdotsfunctor.dart (1), adjustgracexposfunctor.dart (1), adjustharmgrpsspacingfunctor.dart (1), adjustlayersfunctor.dart (1), adjustneumexfunctor.dart (1), adjustossiastaffdeffunctor.dart (1), adjustslursfunctor.dart (1), adjuststaffoverlapfunctor.dart (1), adjustsylspacingfunctor.dart (1), adjusttempofunctor.dart (1), adjusttupletsxfunctor.dart (1), adjustxposfunctor.dart (1), atts_cmnornaments.dart (1), atts_critapp.dart (1), cachehorizontallayoutfunctor.dart (1), calcalignmentpitchposfunctor.dart (1), calcalignmentxposfunctor.dart (1), calcarticfunctor.dart (1), calcbboxoverflowsfunctor.dart (1), calcchordnoteheadsfunctor.dart (1), calcdotsfunctor.dart (1), calcledgerlinesfunctor.dart (1), calcligatureorneumeposfunctor.dart (1), calcslurdirectionfunctor.dart (1), calcspanningbeamspansfunctor.dart (1), calcstemfunctor.dart (1), editortoolkit_cmn.dart (1), editortoolkit_mensural.dart (1), editortoolkit_neume.dart (1), editortoolkit_shared.dart (1), floatingobject.dart (1), ioabc.dart (1), iocmme.dart (1), iogabc.dart (1), iohumdrum.dart (1), iomei.dart (1), iomusxml.dart (1), iopae.dart (1), iovolpiano.dart (1), midifunctor.dart (1), pgfoot.dart (1), pghead.dart (1), phrase.dart (1)

**`definitely_unassigned_late_local_variable`** — 179 ocorrências · 17 arquivos · ERROR

iohumdrum.dart (39), adjustslursfunctor.dart (20), bboxdevicecontext.dart (16), editortoolkit_shared.dart (15), iocmme.dart (14), boundingbox.dart (12), svgdevicecontext.dart (11), zip_file.dart (9), adjustgracexposfunctor.dart (8), devicecontext.dart (7), adjustarpegfunctor.dart (6), calcstemfunctor.dart (6), adjustclefchangesfunctor.dart (5), adjusttupletsxfunctor.dart (4), pugixml.dart (3), adjusttempofunctor.dart (2), beam.dart (2)

**`undefined_operator`** — 161 ocorrências · 14 arquivos · ERROR

iohumdrum.dart (65), pugixml.dart (26), midifunctor.dart (25), convertfunctor.dart (16), zip_file.dart (14), alignfunctor.dart (4), tunings.dart (3), chord.dart (2), beatrpt.dart (1), devicecontext.dart (1), genericlayerelement.dart (1), humlib.dart (1), iomei.dart (1), iopae.dart (1)

**`undefined_function`** — 158 ocorrências · 6 arquivos · ERROR

zip_file.dart (70), pugixml.dart (50), vrv.dart (19), tuningsimpl.dart (8), jsonxx.dart (6), main.dart (5)

**`unused_local_variable`** — 155 ocorrências · 27 arquivos · WARNING

iopae.dart (31), iohumdrum.dart (23), boundingbox.dart (14), editortoolkit_neume.dart (10), zip_file.dart (9), adjustslursfunctor.dart (8), svgdevicecontext.dart (8), tunings.dart (8), pugixml.dart (7), midifunctor.dart (5), findlayerelementsfunctor.dart (4), vrv.dart (4), chord.dart (3), iomei.dart (3), alignfunctor.dart (2), artic.dart (2), iocmme.dart (2), justifyfunctor.dart (2), tuningsimpl.dart (2), adjusttempofunctor.dart (1), adjustxposfunctor.dart (1), beamspan.dart (1), calcdotsfunctor.dart (1), convertfunctor.dart (1), dynam.dart (1), featureextractor.dart (1), iogabc.dart (1)

**`unused_import`** — 143 ocorrências · 50 arquivos · WARNING

calcalignmentpitchposfunctor.dart (18), iovolpiano.dart (17), controlelement.dart (7), customtuning.dart (6), layerelement.dart (5), adjustdotsfunctor.dart (4), altsyminterface.dart (4), clef.dart (4), systemelement.dart (4), adjusttempofunctor.dart (3), areaposinterface.dart (3), beamspan.dart (3), editorial.dart (3), pageelement.dart (3), textelement.dart (3), textlayoutelement.dart (3), adjuststaffoverlapfunctor.dart (2), annotscore.dart (2), beam.dart (2), bracketspan.dart (2), calcspanningbeamspansfunctor.dart (2), chord.dart (2), comparison.dart (2), durationinterface.dart (2), facsimileinterface.dart (2), featureextractor.dart (2), findlayerelementsfunctor.dart (2), linkinginterface.dart (2), offsetinterface.dart (2), pitchinterface.dart (2), plistinterface.dart (2), positioninterface.dart (2), scoredefinterface.dart (2), textdirinterface.dart (2), timeinterface.dart (2), anchoredtext.dart (1), arpeg.dart (1), cachehorizontallayoutfunctor.dart (1), caesura.dart (1), cpmark.dart (1), custos.dart (1), devicecontextbase.dart (1), divline.dart (1), editortoolkit.dart (1), episema.dart (1), fing.dart (1), hairpin.dart (1), keyaccid.dart (1), liquescent.dart (1), scoredef.dart (1)

**`non_abstract_class_inherits_abstract_member`** — 135 ocorrências · 125 arquivos · ERROR

options.dart (8), elementpart.dart (3), pugixml.dart (2), abbr.dart (1), accid.dart (1), add.dart (1), anchoredtext.dart (1), annot.dart (1), annotscore.dart (1), arpeg.dart (1), artic.dart (1), barline.dart (1), beam.dart (1), beamspan.dart (1), beatrpt.dart (1), bracketspan.dart (1), breath.dart (1), btrem.dart (1), caesura.dart (1), chord.dart (1), clef.dart (1), corr.dart (1), course.dart (1), cpmark.dart (1), custos.dart (1), damage.dart (1), del.dart (1), dir.dart (1), divline.dart (1), dot.dart (1), dynam.dart (1), editortoolkit_shared.dart (1), ending.dart (1), episema.dart (1), expan.dart (1), expansion.dart (1), f.dart (1), facsimile.dart (1), fermata.dart (1), fig.dart (1), fing.dart (1), ftrem.dart (1), functor.dart (1), gliss.dart (1), gracegrp.dart (1), graphic.dart (1), grpsym.dart (1), hairpin.dart (1), halfmrpt.dart (1), harm.dart (1), horizontalaligner.dart (1), instrdef.dart (1), keyaccid.dart (1), keysig.dart (1), label.dart (1), labelabbr.dart (1), layer.dart (1), layerdef.dart (1), lem.dart (1), ligature.dart (1), liquescent.dart (1), mdiv.dart (1), measure.dart (1), mensur.dart (1), metersig.dart (1), metersiggrp.dart (1), mnum.dart (1), mordent.dart (1), mrest.dart (1), mrpt.dart (1), mrpt2.dart (1), multirest.dart (1), multirpt.dart (1), nc.dart (1), neume.dart (1), note.dart (1), octave.dart (1), orig.dart (1), oriscus.dart (1), ornam.dart (1), ossia.dart (1), pages.dart (1), pb.dart (1), pedal.dart (1), pitchinflection.dart (1), plica.dart (1), proport.dart (1), quilisma.dart (1), rdg.dart (1), reg.dart (1), reh.dart (1), rend.dart (1), repeatmark.dart (1), rest.dart (1), restore.dart (1), runningelement.dart (1), sb.dart (1), score.dart (1), scoredef.dart (1), section.dart (1), sic.dart (1), slur.dart (1), space.dart (1), staff.dart (1), staffdef.dart (1), staffgrp.dart (1), stem.dart (1), strophicus.dart (1), supplied.dart (1), surface.dart (1), syl.dart (1), syllable.dart (1), symbol.dart (1), system.dart (1), tabdursym.dart (1), tabgrp.dart (1), tempo.dart (1), tie.dart (1), trill.dart (1), tuning.dart (1), tuplet.dart (1), turn.dart (1), unclear.dart (1), verse.dart (1), zone.dart (1)

**`duplicate_definition`** — 118 ocorrências · 17 arquivos · ERROR

humlib.dart (41), jsonxx.dart (29), pugixml.dart (18), iohumdrum.dart (9), boundingbox.dart (4), zip_file.dart (4), iopae.dart (2), tuningsimpl.dart (2), adjustslursfunctor.dart (1), artic.dart (1), devicecontext.dart (1), devicecontextbase.dart (1), editortoolkit_neume.dart (1), editortoolkit_shared.dart (1), iomei.dart (1), midimessage.dart (1), vrv.dart (1)

**`dead_code`** — 83 ocorrências · 14 arquivos · WARNING

accid.dart (32), iohumdrum.dart (10), attalternates.dart (9), editortoolkit_neume.dart (9), svgdevicecontext.dart (5), beam.dart (4), adjustslursfunctor.dart (3), artic.dart (2), custos.dart (2), fig.dart (2), rend.dart (2), humlib.dart (1), pugixml.dart (1), vrv.dart (1)

**`undefined_class`** — 59 ocorrências · 15 arquivos · ERROR

zip_file.dart (15), pugixml.dart (12), iopae.dart (8), convertfunctor.dart (5), artic.dart (4), alignfunctor.dart (2), editortoolkit_neume.dart (2), main.dart (2), object.dart (2), vrv.dart (2), boundingbox.dart (1), iomei.dart (1), svgdevicecontext.dart (1), toolkit.dart (1), tunings.dart (1)

**`non_bool_condition`** — 46 ocorrências · 14 arquivos · ERROR

convertfunctor.dart (7), findlayerelementsfunctor.dart (7), adjustslursfunctor.dart (6), midifunctor.dart (6), iohumdrum.dart (5), editortoolkit_neume.dart (4), devicecontextbase.dart (2), dynam.dart (2), iopae.dart (2), editortoolkit_shared.dart (1), ioabc.dart (1), iogabc.dart (1), pugixml.dart (1), tunings.dart (1)

**`assignment_to_final`** — 39 ocorrências · 5 arquivos · ERROR

iohumdrum.dart (26), iocmme.dart (7), ioabc.dart (3), dynam.dart (2), iopae.dart (1)

**`return_of_invalid_type`** — 35 ocorrências · 24 arquivos · ERROR

pugixml.dart (6), tunings.dart (3), zip_file.dart (3), customtuning.dart (2), graphic.dart (2), accid.dart (1), app.dart (1), artic.dart (1), beam.dart (1), choice.dart (1), devicecontext.dart (1), elementpart.dart (1), fb.dart (1), genericlayerelement.dart (1), justifyfunctor.dart (1), lb.dart (1), main.dart (1), mspace.dart (1), num.dart (1), object.dart (1), ref.dart (1), subst.dart (1), text.dart (1), vrv.dart (1)

**`non_bool_negation_expression`** — 29 ocorrências · 19 arquivos · ERROR

iopae.dart (4), convertfunctor.dart (3), alignfunctor.dart (2), customtuning.dart (2), editortoolkit_neume.dart (2), midifunctor.dart (2), vrv.dart (2), artic.dart (1), boundingbox.dart (1), chord.dart (1), devicecontext.dart (1), ioabc.dart (1), iomei.dart (1), jsonxx.dart (1), justifyfunctor.dart (1), meibasic.dart (1), pugixml.dart (1), resources.dart (1), svgdevicecontext.dart (1)

**`for_in_of_invalid_type`** — 24 ocorrências · 13 arquivos · ERROR

iocmme.dart (7), iopae.dart (3), devicecontext.dart (2), iohumdrum.dart (2), iomei.dart (2), adjustaccidxfunctor.dart (1), adjustslursfunctor.dart (1), bboxdevicecontext.dart (1), chord.dart (1), midifunctor.dart (1), svgdevicecontext.dart (1), toolkit.dart (1), vrv.dart (1)

**`constant_pattern_never_matches_value_type`** — 21 ocorrências · 1 arquivos · WARNING

pugixml.dart (21)

**`non_type_as_type_argument`** — 19 ocorrências · 3 arquivos · ERROR

vrv.dart (17), beamspan.dart (1), main.dart (1)

**`unnecessary_null_comparison`** — 18 ocorrências · 3 arquivos · WARNING

iohumdrum.dart (13), editortoolkit_neume.dart (4), calcstemfunctor.dart (1)

**`undefined_setter`** — 15 ocorrências · 1 arquivos · ERROR

zip_file.dart (15)

**`const_eval_method_invocation`** — 10 ocorrências · 6 arquivos · ERROR

chord.dart (5), boundingbox.dart (1), editortoolkit_neume.dart (1), iopae.dart (1), layerelement.dart (1), pugixml.dart (1)

**`non_bool_operand`** — 9 ocorrências · 6 arquivos · ERROR

convertfunctor.dart (2), iopae.dart (2), midifunctor.dart (2), chord.dart (1), editortoolkit_neume.dart (1), findlayerelementsfunctor.dart (1)

**`invalid_override`** — 6 ocorrências · 2 arquivos · ERROR

bboxdevicecontext.dart (3), svgdevicecontext.dart (3)

**`map_key_type_not_assignable`** — 6 ocorrências · 2 arquivos · ERROR

alignfunctor.dart (3), midifunctor.dart (3)

**`const_eval_property_access`** — 5 ocorrências · 4 arquivos · ERROR

jsonxx.dart (2), bboxdevicecontext.dart (1), devicecontext.dart (1), svgdevicecontext.dart (1)

**`null_check_always_fails`** — 5 ocorrências · 1 arquivos · WARNING

editortoolkit_neume.dart (5)

**`receiver_of_type_never`** — 5 ocorrências · 1 arquivos · WARNING

editortoolkit_neume.dart (5)

**`unnecessary_type_check`** — 4 ocorrências · 2 arquivos · WARNING

fig.dart (2), rend.dart (2)

**`not_a_type`** — 3 ocorrências · 2 arquivos · ERROR

jsonxx.dart (2), iomei.dart (1)

**`positional_field_in_object_pattern`** — 3 ocorrências · 2 arquivos · ERROR

jsonxx.dart (2), iomei.dart (1)

**`referenced_before_declaration`** — 3 ocorrências · 2 arquivos · ERROR

zip_file.dart (2), adjustgracexposfunctor.dart (1)

**`refutable_pattern_in_irrefutable_context`** — 3 ocorrências · 2 arquivos · ERROR

jsonxx.dart (2), iomei.dart (1)

**`nullable_type_in_catch_clause`** — 2 ocorrências · 2 arquivos · WARNING

iocmme.dart (1), iomei.dart (1)

**`pattern_type_mismatch_in_irrefutable_context`** — 2 ocorrências · 1 arquivos · ERROR

pugixml.dart (2)

**`return_of_invalid_type_from_closure`** — 2 ocorrências · 2 arquivos · ERROR

iopae.dart (1), timemap.dart (1)

**`undefined_constructor_in_initializer`** — 2 ocorrências · 2 arquivos · ERROR

iomei.dart (1), iopae.dart (1)

**`invocation_of_non_function_expression`** — 1 ocorrências · 1 arquivos · ERROR

jsonxx.dart (1)

**`main_first_positional_parameter_type`** — 1 ocorrências · 1 arquivos · ERROR

main.dart (1)

**`missing_assignable_selector`** — 1 ocorrências · 1 arquivos · ERROR

pugixml.dart (1)

**`non_type_in_catch_clause`** — 1 ocorrências · 1 arquivos · ERROR

iomei.dart (1)

**`unused_element`** — 1 ocorrências · 1 arquivos · WARNING

humlib.dart (1)
