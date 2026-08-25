# Estado da transpilação do Verovio 6.2.0 — avaliação e o que falta

Este documento avalia **onde o processo de transpilação está hoje** e **o que
falta para a transpilação do Verovio 6.2.0 funcionar**. Ele substitui, como
backlog ativo, `docs/plans/dart-analyze-verovio-6.2.0.md` — cujas 15 famílias
(F1–F15) foram todas executadas (commits `f3cead2`…`32dd1df`) e cujas contagens
estão obsoletas.

Cada família de causa raiz identificada aqui tem um arquivo de prompt
correspondente em `docs/prompts/2026-08-23-NN-*.md`, na ordem de execução
sugerida (§6).

## 1. Entrada e método

Fonte primária: a rodada de `just verovio-diagnosis` de
**2026-08-23T18:01:21Z, commit `32dd1df`** (o `HEAD` de quando esta avaliação
foi escrita), que deixou três artefatos em `.diagnosis/`:

| Arquivo | O que é |
| --- | --- |
| `verovio-6.2.0.md` | métricas da rodada, sem interpretação |
| `verovio-6.2.0.json` | o mesmo, em JSON, **mais** as tabelas completas de bailout por causa |
| `verovio-6.2.0.analyze.json` | saída bruta de `dart analyze --format=json`, 15.775 diagnósticos |
| `dart-package/lib/` | os 317 arquivos `.dart` daquela mesma rodada |

O C++ de origem foi correlacionado extraindo
`test-resources/verovio-version-6.2.0.tar.gz`. A correlação é por
arquivo/classe/linha lida à mão, não automática — o Dart emitido não carrega
comentário de proveniência linha a linha, só o caminho absoluto dentro de cada
mensagem de bailout.

O agrupamento por família foi produzido por script descartável sobre o JSON (não
faz parte do produto) e é **heurístico**: por `code` + padrão da
`problemMessage` + leitura do Dart emitido. Serve para priorizar, não para
prometer contagens exatas de queda.

> **Aviso.** Os caminhos em `location.file` apontam para um diretório temporário
> que não existe mais. Só o sufixo a partir de `lib/` é identidade útil.

### Como reproduzir esta análise

```bash
just package-build          # só se for rodar dentro do Flatpak
just verovio-diagnosis      # 5-6 min; reescreve .diagnosis/ inteiro

# o C++ de origem, para correlacionar (tmp/ é ignorado pelo git)
mkdir -p tmp/verovio-src
tar xzf test-resources/verovio-version-6.2.0.tar.gz -C tmp/verovio-src
```

O agrupamento por `code` sai de um script de dez linhas sobre
`.diagnosis/verovio-6.2.0.analyze.json` (chaves: `diagnostics[].code`,
`.severity`, `.problemMessage`, `.location.file`, `.location.range.start.line`);
as tabelas de bailout já vêm prontas no campo `bailouts` de
`.diagnosis/verovio-6.2.0.json`.

## 2. O que já funciona

Vale registrar, porque o resto do documento é sobre o que falta:

- O pipeline **completo** roda de ponta a ponta sobre o Verovio real, não
  modificado: ingestão via `cmake` → extração de IR via `libclang` (298 unidades
  de compilação, 387s) → emissão Dart → `dart format` → `dart analyze`. Não há
  travamento, estouro de pilha nem unidade que aborte a rodada.
- 1.346 registros e 612 funções livres são lowered; 317 arquivos `.dart` e
  237.366 linhas saem do outro lado.
- **316 dos 317 arquivos são Dart sintaticamente válido.** Um ano atrás isso era
  o objetivo; hoje é o piso.
- As 15 famílias do diagnóstico anterior foram corrigidas de verdade: o merge de
  registros entre unidades de compilação, a decisão global `mixin`/`class`,
  `super.` em chamada qualificada à base, promoção de tipo do Dart, downcast
  preservado, bailout tipado, adaptadores de stdlib, desambiguação de overloads,
  parentização. Os erros de `dart analyze` caíram de **24.791** para **15.775**
  (−36%); os erros propriamente ditos, de 15.738 para 11.902 (−24%); os avisos,
  de 9.053 para 3.873 (−57%).

## 3. Onde o processo está — a leitura honesta

Há duas métricas, e elas contam histórias diferentes.

### 3.1. `dart analyze` — o que o Dart recusa

| Métrica | Valor |
| --- | --- |
| Diagnósticos totais | 15.775 |
| Erros (`ERROR`) | 11.902 |
| Avisos (`WARNING`) | 3.873 |
| `code`s distintos | 45 |
| Arquivos com ao menos um diagnóstico | 289 de 317 |
| Arquivos que não parseiam | 1 (`humlib.dart`) |

Os 15 `code`s mais frequentes:

| `code` | n | arquivos | sev. | família |
| --- | ---: | ---: | --- | --- |
| `argument_type_not_assignable` | 3.001 | 61 | E | T4 / T5 / T12 |
| `unnecessary_non_null_assertion` | 2.472 | 111 | W | T15 |
| `undefined_method` | 2.272 | 103 | E | T3 / T8 / T13 |
| `extra_positional_arguments` | 2.080 | 120 | E | **T2** |
| `invalid_assignment` | 1.587 | 48 | E | T5 / T14 / T12 |
| `undefined_identifier` | 1.245 | 27 | E | **T4** |
| `unused_field` | 717 | 40 | W | T15 (sintoma) |
| `unused_local_variable` | 377 | 55 | W | T10 (sintoma) |
| `unchecked_use_of_nullable_value` | 372 | 5 | E | **T12** |
| `definitely_unassigned_late_local_variable` | 298 | 23 | E | **T10** |
| `unused_import` | 231 | 102 | W | T15 |
| `use_of_void_result` | 226 | 2 | E | T15 |
| `undefined_getter` | 163 | 20 | E | T2 |
| `not_enough_positional_arguments` | 144 | 13 | E | **T11** |
| `invalid_override` | 135 | 87 | E | T1 / T7 |

### 3.2. Bailouts — o que o transpilador desistiu de traduzir

Esta é a métrica que importa mais, e é a que o `dart analyze` **não** vê: um
bailout é Dart perfeitamente válido que lança `UnimplementedError` ou devolve
`_syntaxBridgeUnsupported<T>(...)`. O analisador fica calado; o programa não
funciona.

| Origem | Ocorrências | Causas distintas |
| --- | ---: | ---: |
| Tipo C++ sem mapeamento | 10.764 | 594 |
| Expressão sem lowering | 17.003 | 542 |
| Statement sem lowering | 707 | 27 |

**28.474 bailouts** — quase o dobro do número de diagnósticos do `dart analyze`.
A métrica "Stub (%) = 3,1%" do relatório subestima isso por construção: ela
conta *linhas* de stub, e uma linha traduzida pode conter três bailouts de
expressão embutidos.

As dez maiores causas de bailout de expressão:

| n | Causa |
| ---: | --- |
| 2.630 | `unsupported free operator overload: operator<<` |
| 1.290 | `unsupported std::basic_ostream::operator<< call` |
| 1.181 | conversão implícita `HumdrumToken?` → `String?` |
| 865 | conversão implícita `HumdrumToken` → `String` |
| 702 | conversão implícita do `operator unspecified_bool_type` do pugixml → `bool` |
| 529 | `unsupported free operator overload: operator==` |
| 356 | `unsupported expression cursor kind 119` (`InitListExpr`) |
| 353 / 296 | `std::vector::begin` / `std::vector::end` |
| 332 | `array subscript receiver is not a lowered Dart collection` |
| 258 | `no default value available for this field's type yet` |

E os dez maiores tipos sem mapeamento:

| n | Tipo |
| ---: | --- |
| 6.331 | `std::basic_ostream<char>` (somando as três grafias) |
| 369 | `std::basic_istream<char>` (idem) |
| 560 | `std::basic_regex` + `std::match_results` + `match_flag_type` + `syntax_option_type` |
| ~1.100 | iteradores da libstdc++ (`_List_iterator`, `_Rb_tree_iterator`, `__normal_iterator`, `reverse_iterator`, `_Bit_iterator`) |
| 249 | `std::_Bit_reference` / `const std::_Bit_reference` (`std::vector<bool>`) |
| 100 | `FILE *` |
| ~400 | ponteiro cru como buffer (`int *`, `char **`, `mz_uint64 *`, `const mz_uint16 *`, `size_t *`, …) |

### 3.3. A conclusão

**O produto atravessa o Verovio inteiro, mas o Dart resultante não é executável
nem próximo disso.** O gargalo mudou de lugar: já não é a estrutura (classes,
herança, métodos out-of-line, mixins, nomes — tudo isso está de pé), é

1. **um punhado de fronteiras de biblioteca que não existem** — stream, regex,
   iterador não-vetorial, buffer de ponteiro cru — cada uma responsável por
   milhares de bailouts; e
2. **duas ou três construções da linguagem que são traduzidas de forma
   silenciosamente errada** — listas de inicialização de construtor, cópia por
   valor, `operator[]`.

O item 2 é mais grave que o 1. Um bailout é honesto: ele grita em tempo de
execução. Um construtor cuja lista de inicialização foi descartada devolve um
objeto zerado sem dizer nada — é exatamente a classe de silêncio que o
`AGENTS.md` proíbe.

## 4. As famílias

Cada família diz em qual categoria a proposta se encaixa:

1. **Correção local de lowering/emissão** — o dado já está no IR.
2. **Mais informação na ingestão** — o dado não está sendo extraído do C++.
3. **Fase nova, com visão do projeto inteiro.**

---

### T1 — Listas de inicialização de construtor são descartadas inteiras

**Categoria: 2.** *Não aparece no `dart analyze` como erro* (a não ser
indiretamente, 105 vezes) — foi encontrada lendo o Dart emitido, e é a mais
grave do documento.

**Sintoma.** `include/vrv/devicecontextbase.h:208`:

```cpp
Point(int xx, int yy) : x(xx), y(yy) {}
```

vira, em `.diagnosis/dart-package/lib/devicecontextbase.dart:284`:

```dart
Point.ctor2(int xx, int yy) {
}
```

`x` e `y` nunca recebem `xx` e `yy`. **Todo `Point(a, b)` do Verovio produz
`(0, 0)`.** O mesmo em `pugixml.dart:493` (`xml_node.ctor2(xml_node_struct? p)`
nunca atribui `root`, o que sozinho quebra a árvore XML inteira),
`pugixml.dart:155` (`xml_attribute`), e em 60 construtores que saem com
**parâmetros e corpo completamente vazio** no pacote emitido. O total de
construtores afetados é maior: qualquer construtor que combine lista de
inicialização *e* corpo perde só a lista, e sai parecendo plausível.

Uma varredura do C++ de origem encontra **724 linhas** com forma de lista de
inicialização (`) : Nome(`) em `src/`, `include/` e `libmei/`.

**Causa raiz.** `lower::cpp::lower_constructor`
(`crates/server/src/lower/cpp.rs:1108`) lê exclusivamente
`find_compound_stmt_child(cursor)` — o corpo. `ir::Constructor`
(`crates/server/src/ir/mod.rs:886`) não tem campo nenhum para inicializadores.
O comentário em `cpp.rs:1480-1486` afirma que
`CXCursor_CXXCtorInitializer` "não existe na API pública do libclang" — o que é
verdade e **não** implica que o dado seja inalcançável: o `libclang` expõe cada
inicializador *escrito* como filho direto do cursor do construtor, na forma
`CXCursor_MemberRef` (campo) ou `CXCursor_TypeRef` (base), seguido do cursor da
expressão de inicialização, antes do `CompoundStmt`.

**Erros que isto explica.** 105 `implicit_super_initializer_missing_arguments`
(a lista `: DocFunctor(doc)` é justamente o `super(...)` que falta — 61 dos 105
são `DocFunctor`) e uma parte de `invalid_override`. O resto do dano é
silencioso.

Prompt: `docs/prompts/2026-08-23-01-listas-de-inicializacao-de-construtor.md`

---

### T2 — Cópia por valor chama um construtor posicional que não existe

**Categoria: 1.**

**Sintoma.** 2.080 `extra_positional_arguments`, o terceiro `code` mais
frequente, e o mais concentrado: 1.329 deles são "0 esperados, 1 encontrado".
`atts_analytical.dart:26`:

```dart
bool ReadHarmAnl(xml_node element, [bool removeAttr = true]) {
  element = xml_node(element.root);   // ← xml_node() aceita 0 argumentos
```

e `iohumdrum.dart:150`:

```dart
void setMeterBottom(HumNum meterbot) {
  meterbot = HumNum(meterbot._top, meterbot._bot);  // ← e `_top` é privado de outra biblioteca
```

**Causa raiz.** `lower::cpp::collect_params_with_clone_prelude`
(`crates/server/src/lower/cpp.rs:2819`) implementa a semântica de cópia de um
parâmetro por valor emitindo `p = Tipo(p.campo1, p.campo2, …)` —
`ir::Expr::RecordConstruct`, que `emit::dart` (`emit/dart.rs:3832`) imprime
literalmente como `Tipo(args)`. Isso pressupõe o **construtor posicional
sintético**, que `emit_record` só emite quando `record.constructors.is_empty()`
(o comentário em `emit/dart.rs:1096-1112` é explícito: "as duas formas não se
misturam no mesmo registro"). Para qualquer registro com construtor próprio — a
maioria — a chamada gerada não existe.

O segundo dano é de privacidade: a cópia campo a campo lê campos `private` de um
registro que mora em outra biblioteca Dart. Daí ~120 `undefined_getter`
(`_top`/`_bot` de `HumNum`, `_m_numerator`/`_m_denominator` de `Fraction`).

**Proposta.** A cópia por valor não deve ser expressa como uma construção: deve
ser um método gerado **no próprio registro** (`Tipo copy()` ou
`Tipo.copiedFrom(Tipo other)`), emitido para todo registro copiável, dentro da
biblioteca onde os campos são visíveis. O prelúdio passa a emitir
`p = p.copy();`.

Prompt: `docs/prompts/2026-08-23-02-copia-por-valor-sem-construtor-posicional.md`

---

### T3 — `operator[]` e `operator!=` viram `unsupportedOperator`

**Categoria: 1.** A correção mais barata do documento por unidade de erro
eliminada.

**Sintoma.** 1.101 `undefined_method` sobre `unsupportedOperator` — 730 só sobre
`HumdrumFile`, o idioma `infile[i]` do humlib. Mais 44 `duplicate_definition`,
todos porque dois operadores diferentes da mesma classe colapsam no mesmo nome
`unsupportedOperator`.

**Causa raiz.** `lower::cpp::dart_operator_bridge_name`
(`crates/server/src/lower/cpp.rs:7728`) mapeia 20 operadores para nomes-ponte e
manda todo o resto para `"unsupportedOperator"`. Nesse "resto" estão dois
operadores que o **Dart declara nativamente**:

- `operator[]` → `operator [](i)` (e `operator []=(i, v)` para a forma de
  escrita);
- `operator!=` → não precisa de nada: o Dart deriva `!=` de `==`
  automaticamente, então o call site deve virar `!(a == b)` e a declaração deve
  simplesmente não ser emitida.

Prompt: `docs/prompts/2026-08-23-03-operadores-indice-e-diferenca.md`

---

### T4 — `std::string` tratada como sequência de bytes, com `npos` vazando

**Categoria: 1.**

**Sintoma.** ~2.370 erros, três formas do mesmo problema:

- **798 `Undefined name 'basic_string'`** — `std::string::npos` é emitido como
  `basic_string.npos` (`docselection.dart:44`:
  `… .indexOf(…) != basic_string.npos`);
- **769 `The argument type 'Uint8List' can't be assigned to the parameter type
  'int'`** — `emit::dart` (`emit/dart.rs:3902`) imprime
  `Expr::StringByteIndexOf` como `utf8.encode(a).indexOf(utf8.encode(b))`, e
  `Uint8List.indexOf` espera **um elemento** (`int`), não uma lista;
- **666 `The argument type 'int' can't be assigned to the parameter type
  'String'`** — o mesmo caminho com um `char` (que o bridge mapeia para `int`)
  no lugar do agulha: `utf8.encode(34)`. E `output = output + 45` em
  `humlib.dart:1172`, de `std::string += char`.

**Causa raiz.** O bridge escolheu representar `std::string` como `String` do
Dart mas indexá-la em **bytes** (`StringByteLength`, `StringByteIndexOf`,
`StringByteAt`) para preservar a semântica de `s[i]` do C++. A escolha é
defensável; a implementação está incompleta: `npos` não tem mapeamento, `find`
com agulha de mais de um byte não tem tradução correta, e a conversão
`char` ↔ `String` só acontece em alguns braços.

Prompt: `docs/prompts/2026-08-23-04-string-como-bytes-npos-e-find.md`

---

### T5 — `const char*` é `String?` e `std::string` é `String`, sem ponte entre os dois

**Categoria: 1.**

**Sintoma.** 1.220 erros (1.043 `argument_type_not_assignable` + 177
`invalid_assignment`), todos `String?` onde se esperava `String`.
`atts_analytical.dart:29`:

```dart
SetForm(StrToHarmAnlForm(element.attributeNullableStringConst('form').value()));
```

`value()` do pugixml devolve `const char*` → `String?`; `StrToHarmAnlForm`
recebe `std::string` → `String`.

**Causa raiz.** Em C++ existe uma conversão implícita `const char*` →
`std::string` (o construtor de `std::string`), e ela é onde o contrato
"nunca nulo" é assegurado. O lowering apaga essa conversão em vez de
materializá-la.

Prompt: `docs/prompts/2026-08-23-05-fronteira-char-pointer-e-string.md`

---

### T6 — `std::ostream`/`std::istream` não têm fronteira nomeada

**Categoria: 1** (o mapeamento) **+ 3** (a decisão de qual adaptador o produto
oferece). **A maior família do documento em volume de bailout.**

**Sintoma.** ~10.000 bailouts:

| n | Causa |
| ---: | --- |
| 6.331 | tipo `std::basic_ostream<char>` sem mapeamento |
| 2.630 | `unsupported free operator overload: operator<<` |
| 1.290 | `unsupported std::basic_ostream::operator<< call` |
| 369 | tipo `std::basic_istream<char>` sem mapeamento |
| 227 | conversão implícita `Str` → `std::basic_ostream` |

**Causa raiz.** `lower::cpp` reconhece exatamente dois casos:
`lower_ostream_insertion_chain` (`cpp.rs:7898`), para cadeias que começam nos
globais `std::cout`/`std::cerr`, e `lower_stringstream_insertion_chain`
(`cpp.rs:8038`), para uma variável local `std::stringstream`. Tudo o mais — e o
Verovio é feito disso — cai fora:

- `std::ostream &output` como **parâmetro** (o idioma canônico do humlib:
  `void printXml(ostream& out)`, `ostream& operator<<(ostream&, const X&)`);
- `std::ofstream` para arquivo;
- `std::ostream` guardado em campo.

Não há nenhum tipo Dart para `std::ostream`, então o parâmetro inteiro vira
`SyntaxBridgeOpaque` e o método inteiro vira bailout.

**Proposta.** Uma fronteira nomeada em `syntax_bridge_support.dart` — um
`SyntaxBridgeOutputStream` com `write(String)`, com implementações para stdout,
stderr, `StringBuffer` e arquivo — mapeada a partir de `std::ostream` e
derivadas; `operator<<` vira `out.write(...)`. O mesmo do lado da entrada.
Isso é o que o `AGENTS.md` chama de "fronteira externa explicitamente modelada".

Prompt: `docs/prompts/2026-08-23-06-fronteira-de-stream.md`

---

### T7 — Operadores de conversão e heranças de tipo de biblioteca

**Categoria: 2.**

**Sintoma.** ~3.800 bailouts, três sub-casos com a mesma raiz:

- **1.000** — `unsupported implicit conversion from Callback { … } to Bool`, o
  idioma *safe bool* do pugixml (`if (element.attribute("func"))`, onde
  `xml_attribute::operator unspecified_bool_type()` é um ponteiro-para-membro).
  Isso sozinho transforma **toda** função `Read*`/`Write*` de `libmei/dist/`
  em bailout — e são milhares.
- **2.046** — `HumdrumToken` → `std::string`. `class HumdrumToken : public
  std::string, public HumHash` (`humlib.h`). O `lower::cpp` explicitamente
  **filtra** bases que não são `Type::Record` (`cpp.rs:1487`, com um comentário
  longo justificando), então a herança some e todo uso de um `HumdrumToken`
  como string vira bailout.
- **765** — `GridStaff`/`GridSlice`/`GridPart`/`GridMeasure`/`HumGrid` →
  `List<…>`. Mesma história: essas classes herdam de `std::vector<…>`.

**Proposta.** Duas peças: (a) lowerar `CXCursor_ConversionFunction` como um
método-ponte nomeado e usá-lo onde a conversão implícita hoje falha; (b) para a
herança de tipo de biblioteca, um adaptador nomeado — o registro ganha um campo
que **é** a `String`/`List<T>` da base, e todo uso do objeto como base vira
acesso a esse campo, em vez do apagamento atual.

Prompt: `docs/prompts/2026-08-23-07-conversoes-definidas-pelo-usuario.md`

---

### T8 — Templates de membro não são monomorfizados

**Categoria: 1.**

**Sintoma.** 251 `undefined_method` (`get` 133, `has` 118, sobre
`JsonxxObject`) e 161 bailouts `call to a member operator template
instantiation — not yet monomorphized`. `docselection.dart:39`:

```dart
if (json.has('measureRange')) { m_measureRange = json.get('measureRange'); }
```

`jsonxx::Object::has<std::string>()` e `get<std::string>()` são templates de
membro, e nenhum dos dois foi emitido.

**Causa raiz.** `function_catalog` já monomorfiza templates de **função livre**
(via `clang_getSpecializedCursorTemplate` + `monomorphized_template_name`,
`function_catalog.rs:3319-3340`), mas o mesmo caminho não é percorrido para
métodos-template de um registro.

Prompt: `docs/prompts/2026-08-23-08-templates-de-membro.md`

---

### T9 — Iteradores não-vetoriais e `std::vector<bool>`

**Categoria: 1.**

**Sintoma.** ~2.000 bailouts e 117 erros. A tarefa 13 do lote anterior resolveu
o idioma sobre `std::vector` e criou o `SyntaxBridgeListCursor`; sobraram:

| n | Tipo/causa |
| ---: | --- |
| 232 + 122 + 117 + 109 | `std::_List_iterator::operator*` / `->` / `++`, `std::_Rb_tree_iterator::operator->` |
| 353 + 296 + 109 + 80 | `vector::begin`/`vector::end`/`list::begin`/`basic_string::begin` ainda em posições não reconhecidas |
| 249 | `std::_Bit_reference` (`std::vector<bool>`) |
| 76 | `The operator '+' isn't defined for the type 'SyntaxBridgeListCursor<T>'` — aritmética de iterador |
| 67 + 45 | `std::reverse_iterator` |

Prompt: `docs/prompts/2026-08-23-09-iteradores-de-lista-mapa-e-vector-bool.md`

---

### T10 — Parâmetro de saída: o local ainda é `late` sem valor

**Categoria: 1.**

**Sintoma.** 298 `definitely_unassigned_late_local_variable` (era 179 antes da
tarefa 10 — a ponte passou a funcionar em mais lugares, e o sintoma seguiu
junto). `adjustclefchangesfunctor.dart:76-81`:

```dart
late int nextLeft;
late int nextRight;
…
(nextLeft, nextRight) = nextAlignment.GetLeftRightListIntIntIntListClassIdConst(ns, nextLeft, nextRight);
```

A ponte de tupla funcionou; o que sobrou é que (a) a declaração de
`int nextLeft, nextRight;` sem inicializador virou `late` sem valor, e (b) a
chamada ainda **passa** os dois como argumentos de entrada.

**Proposta.** Um local declarado sem inicializador e usado como argumento de uma
chamada com ponte de out-param recebe o valor neutro do tipo (`0`, `false`,
`''`), nunca `late`. `AGENTS.md`: nunca deixar `late` sem escrita.

Prompt: `docs/prompts/2026-08-23-10-out-params-late-sem-valor.md`

---

### T11 — Argumentos default vivem na declaração e são perdidos no merge

**Categoria: 1.** Pequena, precisa, barata.

**Sintoma.** ~120 dos 144 `not_enough_positional_arguments`.
`humlib.h:4047` declara

```cpp
static std::string durationToRecip(HumNum duration, HumNum scale = 1);
```

e `humlib.cpp:5208` define `string Convert::durationToRecip(HumNum duration,
HumNum scale) { … }` — sem repetir o default, como o C++ exige. O merge
(`finish_function_catalog`) prefere corretamente a versão *com corpo*, e com ela
adota a lista de parâmetros **sem** os defaults. Resultado:
`static String durationToRecip(HumNum duration, HumNum scale)`, e os 20 call
sites de um argumento quebram.

Os 49 `stoi` são o mesmo fenômeno visto pelo lado de fora: `std::stoi` não está
na tabela de adaptadores, sai como mock externo com 3 parâmetros obrigatórios.

Prompt: `docs/prompts/2026-08-23-11-argumentos-default-no-merge.md`

---

### T12 — Ponteiro cru como buffer

**Categoria: 2.**

**Sintoma.** 372 `unchecked_use_of_nullable_value` (279 em `pugixml.dart`, 85 em
`zip_file.dart`), 332 bailouts `array subscript receiver is not a lowered Dart
collection`, 303 bailouts `assignment target is not a simple local variable
(index assignment not supported yet)`, 129 `address-of requires a representable
nullable reference`, mais ~400 tipos `int *` / `char **` / `mz_uint16 *` /
`size_t *` sem mapeamento.

São dois arquivos (`pugixml.cpp`, `zip_file.hpp`/miniz) escritos em C com
ponteiros crus, aritmética de ponteiro e escrita por índice.

**Proposta.** Um adaptador nomeado de buffer (uma visão sobre `Uint8List` com
deslocamento) e o reconhecimento dos três idiomas — `*p`, `p[i]`, `p[i] = v`,
`p++` — sobre ele. É a família com maior risco de virar poço sem fundo; o prompt
delimita o escopo.

Prompt: `docs/prompts/2026-08-23-12-buffers-de-ponteiro-cru.md`

---

### T13 — `std::regex`

**Categoria: 1.**

**Sintoma.** ~560 bailouts de tipo (`basic_regex` 179, `match_results` 135,
`match_flag_type` 127, `syntax_option_type` 118) e 21 `undefined_method`
(`basic_regex`, `regex_search`, `regex_replace`). Concentrado em `humlib` e
`iohumdrum`, onde é a ferramenta principal de parsing.

**Proposta.** Mapeamento direto: `std::regex` → `RegExp`, `std::regex_search` →
`RegExp.firstMatch`, `std::regex_replace` → `String.replaceAll(RegExp, …)`,
`std::smatch` → `RegExpMatch?`. As sintaxes ECMAScript de ambos são compatíveis
no subconjunto usado.

Prompt: `docs/prompts/2026-08-23-13-regex.md`

---

### T14 — Resíduos de emissão (limpeza)

**Categoria: 1**, itens independentes, agrupados porque cada um é uma correção
local:

| Item | `code` | n | O que fazer |
| --- | --- | ---: | --- |
| `m[k]++` vira `m.putIfAbsent(k, () => 0)++` | *parse error* | 4 | O **único arquivo que não parseia** do pacote (`humlib.dart`) |
| `!` redundante | `unnecessary_non_null_assertion` | 2.472 | A promoção da tarefa 04 não alcança condição composta, `for`, `while` nem campo local |
| `import` não usado | `unused_import` | 231 | Emitir só o que é referenciado |
| bailout tipado `void` em posição de valor | `use_of_void_result` | 226 | `_syntaxBridgeUnsupported<void>` dentro de um literal de lista (`humlib.dart:20593`) |
| narrowing `double`→`int` | `invalid_assignment` | ~130 | A conversão da tarefa 11 não cobre inicialização de campo nem argumento |
| `dead_code` | `dead_code` | 40 | `if (false)`, terminador redundante |
| `for` sobre tipo não iterável | `for_in_of_invalid_type` | 21 | resto de T9 |
| `goto`/label | *bailout* | 74 | `unsupported statement cursor kind 210/201` |
| lambda | *bailout* | 145+ | `unsupported expression cursor kind 144` |
| `InitListExpr` aninhado | *bailout* | 356 | `{{a,b},{c,d}}` |
| `const_cast` | *bailout* | 107 | `unsupported expression cursor kind 127` — é invólucro transparente |

Prompt: `docs/prompts/2026-08-23-14-limpeza-de-emissao.md`

#### Medicoes da execucao de T14

**Tarefa 14.7 (`unused_field`, 2026-08-24).** O diagnostico executado no
Flatpak, no commit `8a71d0e`, encontrou 55 avisos em 19 arquivos, e nao os 717
da estimativa que originou o prompt. A inspecao cruzada das declaracoes Dart
com o Verovio 6.2.0 nao encontrou o caso (a): nao ha leitura explicita de um
campo privado gerado por uma subclasse em outra biblioteca Dart. Os campos
amostrados continuam usados no C++ (`m_currentMeasure`, `m_isOtherLayer`,
`m_classIds` e os campos de `SetScoreDefFunctor`), mas seus leitores estao em
metodos que ainda contem bailouts ou nao sobreviveram integralmente ao
lowering. Portanto, o residuo e predominantemente o caso (b), nao campo morto
do modelo nem regressao de visibilidade. Nenhum campo foi removido e o aviso
nao foi silenciado; ele deve cair conforme os corpos consumidores forem
materializados.

**Tarefa 14.8 (`goto`/rotulo, 2026-08-24).** A fonte do Verovio 6.2.0 contem
13 `goto` textuais nos dois focos do diagnostico. Em `zip_file.hpp`, 11 saltos
sao para frente (um para `common_exit` e dez para `handle_failure`); em
`pugixml.cpp`, os dois saltos, para `LOC_ATTRIBUTES` e `LOC_TAG`, sao para
tras. Assim, 11/13 (84,6%) pertencem ao subconjunto potencialmente traduzivel
e 2/13 (15,4%) exigem reestruturacao de fluxo. Os 64 bailouts de `GotoStmt` e
10 de `LabelStmt` contam expansoes e unidades de compilacao, nao 74 comandos
distintos na fonte. A implementacao do subconjunto para frente nao e uma
correcao local: requer representar blocos rotulados no IR, resolver o alvo e
provar que o salto nao atravessa um escopo. Por isso ela fica registrada como
tarefa propria; os dois saltos para tras continuam explicitamente fora desse
subconjunto.

**Fechamento da T14 (2026-08-24/25).** A rodada final de
`just verovio-diagnosis`, dentro do Flatpak, foi executada no commit
`bddc68f`: 317 arquivos Dart, 10.796 erros, 4.317 avisos e 8 arquivos que não
parseiam. O ponto de partida do prompt deixou de ser diretamente comparável
durante a execução: as tarefas anteriores do mesmo lote materializaram muitos
corpos que antes eram bailouts integrais, aumentando a superfície que o
analisador consegue inspecionar. Por isso a tabela abaixo separa a correção da
causa pedida do resíduo global atual, em vez de atribuir todo o `code` à mesma
causa.

| Item | Resultado final | Leitura do resíduo |
| --- | ---: | --- |
| 14.1 | 0 `illegal_assignment_to_non_assignable`; 8/317 não parseiam | A forma inválida `putIfAbsent(...)++` foi eliminada. Os oito arquivos atuais falham por tipos ponteiro-duplo impressos como `T??`, uma fronteira de tipos distinta. |
| 14.2 | 4 `use_of_void_result` | A família de 226 stubs `void` em literais tipados foi eliminada. Os quatro resíduos são colisões nome de método/construtor (`OpenTie`/`CloseTie`/`OpenSlur`/`CloseSlur`) em `iomusxml.dart`. |
| 14.3 | 3.189 `unnecessary_non_null_assertion` | Os três fluxos pedidos têm regressões próprias, incluindo guardas negados e cadeias `||`. Na superfície atual, a contagem observada durante T14 caiu de 4.604 para 3.189; o restante inclui formas de fluxo/tipagem fora desses três padrões. |
| 14.4 | 7 `unused_import` | Caiu de 132 na primeira rodada desta execução para 7. Um é consequência do arquivo sintaticamente inválido `getopt_ext.dart`; os seis restantes exigem tornar a coleta de dependências emitidas totalmente orientada a símbolos, pois adaptadores e homônimos ainda deixam metadados de USR sem uso textual. |
| 14.5 | 39 `dead_code` | Caiu de 55 para 39. Literais falsos e caudas após salto direto ou `if/else` terminal são removidos; os resíduos observados dependem de bailouts/out-params que mudaram o fluxo (ou de condições que o Dart prova constantes), e removê-los isoladamente esconderia perda semântica. |
| 14.6 | 13 `invalid_assignment` `double`→`int` | Inicializadores de campo, argumentos, retornos e atribuições compostas aplicam a conversão na fronteira. Dez resíduos estão no `pugixml.dart` que não parseia e são diagnósticos de recuperação sobre aritmética inteira; os outros três envolvem o modelo numérico de tipos inteiros largos/expressões condicionais. |
| 14.7 | 46 `unused_field` | A medição original abaixo permanece válida; a queda adicional de 55 para 46 veio da materialização de lambdas, sem remoção/silenciamento de campos. |
| 14.8 | 64 `GotoStmt`, 9 `LabelStmt` | 11/13 saltos textuais são para frente e 2/13 para trás; a implementação correta requer IR de bloco rotulado e continua tarefa própria. |
| 14.9 | 0 bailouts `expression cursor kind 144` | Lambdas comuns foram materializadas; init-captures exóticas continuam bailouts específicos, não o bailout genérico de cursor. |
| 14.10 | 23 bailouts IR `expression cursor kind 119`, 0 stubs textuais emitidos | Listas, maps, pares, tuples e agregados aninhados comuns foram materializados (356→23). Os 23 restantes só aparecem em corpos posteriormente colapsados por outro bailout e precisam de outras formas de tipo inicializável; não foram apagados nem retipados como `dynamic`. |
| 14.11 | 0 bailouts `expression cursor kind 127` | `const_cast` é transparente; os testes de downcast continuam passando. |

As três contagens globais de bailout na rodada final foram 1.621 tipos, 5.852
expressões e 618 statements. Os complementos exclusivos de emissão mantiveram
as três contagens estáveis; a canonicalização de agregados no LLVM 21 reduziu
expressões de 5.863 para 5.852. Nenhuma categoria subiu silenciosamente no
fechamento.

## 5. O que "funcionar" quer dizer, e o que ainda não existe para medir isso

Nenhuma das tarefas acima prova que o Dart emitido **faz a mesma coisa** que o
C++ *no Verovio*. Hoje o produto tem três provas, e todas param antes disso:

- `dart analyze` limpo (US-9, parcial) — prova que o Dart é *tipável*;
- os goldens de `examples/E01`…`E13` — provam que a saída textual de casos
  pequenos conhecidos não regride;
- o **oráculo comportamental** de `crates/server/tests/conversion_examples.rs`,
  que executa o C++ e o Dart de cada exemplo com os casos de
  `examples/EN/oracle/cases.json` e compara os resultados. Isso é US-10 de
  verdade — mas na escala de um exemplo de dezenas de linhas, não de um projeto
  de 298 unidades de compilação.

Falta o degrau que responde "o Verovio transpilado funciona?": um oráculo na
escala do Verovio. O critério prático mais próximo é pegar um arquivo MEI de
exemplo, rodar `verovio` em C++ e o pacote Dart emitido, e comparar o SVG
produzido. Isso não é uma tarefa deste lote, mas é o marco que deve ser
declarado logo depois dele — sem ele, "faltam N erros" não é a mesma coisa que
"falta N para funcionar".

Uma segunda lacuna de medição: o pacote emitido tem `lib/` e nada mais — não há
`pubspec.yaml` nem ponto de entrada. `dart analyze` roda sobre ele, mas nada
executa.

## 6. Ordem sugerida de execução

Ordenada por alavancagem: quanto cada correção destrava, não só quanto o `code`
isolado conta. As três primeiras são as que mudam o Dart de "tipável" para
"possivelmente correto"; as três seguintes são as de maior volume bruto.

| # | Prompt | Família | Alvo |
| ---: | --- | --- | --- |
| 01 | `2026-08-23-01-listas-de-inicializacao-de-construtor.md` | T1 | 105 erros + ~724 construtores silenciosamente errados |
| 02 | `2026-08-23-02-copia-por-valor-sem-construtor-posicional.md` | T2 | ~2.200 erros |
| 03 | `2026-08-23-03-operadores-indice-e-diferenca.md` | T3 | ~1.145 erros |
| 04 | `2026-08-23-04-string-como-bytes-npos-e-find.md` | T4 | ~2.370 erros |
| 05 | `2026-08-23-05-fronteira-char-pointer-e-string.md` | T5 | ~1.220 erros |
| 06 | `2026-08-23-06-fronteira-de-stream.md` | T6 | ~10.000 bailouts |
| 07 | `2026-08-23-07-conversoes-definidas-pelo-usuario.md` | T7 | ~3.800 bailouts |
| 08 | `2026-08-23-08-templates-de-membro.md` | T8 | 251 erros + 161 bailouts |
| 09 | `2026-08-23-09-iteradores-de-lista-mapa-e-vector-bool.md` | T9 | 117 erros + ~2.000 bailouts |
| 10 | `2026-08-23-10-out-params-late-sem-valor.md` | T10 | 298 erros (+377 avisos) |
| 11 | `2026-08-23-11-argumentos-default-no-merge.md` | T11 | ~120 erros |
| 12 | `2026-08-23-12-buffers-de-ponteiro-cru.md` | T12 | 372 erros + ~760 bailouts |
| 13 | `2026-08-23-13-regex.md` | T13 | ~580 bailouts |
| 14 | `2026-08-23-14-limpeza-de-emissao.md` | T14 | ~3.100 diagnósticos + 1 arquivo que não parseia |

Depois de **cada** prompt, rodar `just verovio-diagnosis` de novo e comparar:

- o agrupamento por `code` do `dart analyze` — os alvos devem cair, e nenhum
  outro grupo deve subir sem justificativa registrada;
- as **três contagens de bailout** de `.diagnosis/verovio-6.2.0.md` (tipo,
  expressão, statement). Para as tarefas 06 a 13 esta é a métrica principal, não
  o `dart analyze`: converter bailout em tradução real costuma *criar*
  diagnósticos novos antes de eliminá-los, e isso é progresso, não regressão —
  desde que registrado.

As contagens-alvo acima são estimativas da atribuição heurística de §3, não
promessas.
