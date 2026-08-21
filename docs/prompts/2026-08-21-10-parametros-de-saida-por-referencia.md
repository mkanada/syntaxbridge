# Tarefa 10 — Parâmetro de saída por referência vira `late` local que ninguém escreve

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido, silêncio proibido).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F8**. Este prompt é autocontido.

**Execute a tarefa 01 antes desta e re-meça.** Há forte suspeita de que a causa
imediata seja a mesma: a ponte de out-param precisa da declaração *e* da
definição do método, e o merge quebrado que a tarefa 01 conserta entrega só uma
das duas. Se depois da 01 os erros sumirem, esta tarefa vira só a verificação.

## A causa raiz aparente

C++ escreve em parâmetros por referência:

```cpp
void StaffAlignment::GetLeftRight(int staffN, int &minLeft, int &maxRight) const;
```

Em Dart, `int` é passado por valor: uma chamada não pode escrever de volta numa
variável do chamador.

O produto **já tem** a ponte certa: `lower::cpp::apply_out_param_bridge`
transforma um parâmetro de saída num valor de retorno em tupla, e
`ir::Stmt::TupleAssign` + `emit::dart` (veja `TUPLE_ASSIGN_TEMP` e
`is_tuple_assign_discard`, `crates/server/src/emit/dart.rs`, por volta de
2660-2690, com doc comments registrando a rodada 20 que a introduziu) emitem a
desestruturação no chamador.

Ela não está sendo aplicada a estes casos. O que sai é um `late` local que
nunca recebe valor:

`.diagnosis/dart-package/lib/adjustarpegfunctor.dart:46-49`:

```dart
late int minTopLeft;
late int maxTopRight;
topNote!.GetAlignment()!.GetLeftRight(staffN, minTopLeft, maxTopRight);
_m_alignmentArpegTuples.add(make_tuple(…, minTopLeft, …));
//                                        ^ definitely_unassigned_late_local_variable
```

A chamada passa os dois por valor, não escreve nada, e o `late` continua sem
valor — o que o Dart detecta estaticamente.

## A evidência

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791
diagnósticos):

- `definitely_unassigned_late_local_variable` — **179** ocorrências, 17
  arquivos: `iohumdrum.dart` (39), `adjustslursfunctor.dart` (20),
  `bboxdevicecontext.dart` (16), `editortoolkit_shared.dart` (15),
  `iocmme.dart` (14), `boundingbox.dart` (12), `svgdevicecontext.dart` (11),
  `zip_file.dart` (9)…
- Nomes mais frequentes: `output` (33), `y` (10), `x` (9), `intersectionLeft`
  (8), `intersectionRight` (8), `num` (7), `numbase` (7), `minLeft` (6),
  `file_index` (5).
- `unused_local_variable` — **155** ocorrências, 27 arquivos. É o mesmo
  fenômeno visto do outro lado: a local existe, ninguém escreve nela, e em
  alguns casos ninguém a lê tampouco.

Exemplo adicional, `.diagnosis/dart-package/lib/zip_file.dart:988` →
`return ret;`, com `late int ret;` declarado antes e nunca atribuído.

O mesmo padrão aparece com ponteiros: `.diagnosis/dart-package/lib/pugixml.dart`,
dentro de `xml_allocator.allocate_string`:

```dart
late xml_memory_page? page;
xml_memory_string_header? header = allocate_memory(full_size, page);
```

## Onde mexer

- `crates/server/src/lower/cpp.rs` — `apply_out_param_bridge` e o ponto que
  decide se um parâmetro qualifica como out-param. Comece descobrindo **por que**
  ele não dispara nestes casos: parâmetro `const T&` (entrada, não saída) versus
  `T&` (saída) versus `T*&`; método versus função livre; declaração no header
  versus definição no `.cpp`.
- `crates/server/src/ir/mod.rs` — `Stmt::TupleAssign`, `Param`.
- `crates/server/src/emit/dart.rs` — o caminho de `Stmt::TupleAssign`, se a
  forma emitida precisar mudar.

Regra que **não** pode ser violada: um `late` local sem escrita é erro de
compilação, não um bailout honesto. Se para algum caso a ponte de tupla não
servir, a saída é um bailout explícito (`Stmt::Unsupported`), não deixar o
`late` de pé.

Se o `late` sobreviver como forma emitida em algum caminho legítimo, ele
precisa ser inicializado com um valor neutro do tipo em vez de `late`, e a
chamada precisa ser a forma de tupla.

## Método

TDD, conforme `AGENTS.md`:

1. **Primeiro, re-meça.** Rode `just verovio-diagnosis` depois da tarefa 01 e
   veja quanto de `definitely_unassigned_late_local_variable` sobrou. Se for
   zero, esta tarefa está feita e o resumo deve dizer isso, com o número.
2. Se sobrar: escreva um teste que falha reproduzindo o caso restante. Fixture
   mínimo: um método declarado num header com `void f(int &a, int &b)` e
   definido num `.cpp`, chamado de outro arquivo. Compare com um fixture que
   **funciona** hoje (procure nos testes existentes o que cobre
   `apply_out_param_bridge` — a rodada 20 deixou testes) para isolar a
   diferença.
3. Implemente até passar.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `definitely_unassigned_late_local_variable` — de **179** para **zero**.
- `unused_local_variable` — de **155** para uma fração pequena (o resíduo
  legítimo são locais que o C++ original também não usava).
- Nenhum `late` sem inicialização no pacote emitido. Verificação direta:
  `grep -rn "^\s*late " .diagnosis/dart-package/lib/*.dart` — cada ocorrência
  precisa ter uma escrita provável antes do primeiro uso.
- Nenhum `code` novo. A forma de tupla tem armadilhas de sintaxe próprias
  (`emit::dart::tuple_assign_needs_temp_block` documenta uma delas: `!` não
  cabe dentro de um padrão de atribuição) — se
  `pattern_type_mismatch_in_irrefutable_context`,
  `positional_field_in_object_pattern` ou
  `refutable_pattern_in_irrefutable_context` subirem, é aí.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso possível: um método com muitos
parâmetros de saída (o corpus tem
`6 positional arguments expected by 'write'`) vira uma tupla larga, e a
alternativa — um objeto de resultado nomeado — muda a forma da API gerada.
Se a tupla ficar impraticável em algum caso real, pergunte.

Dificuldade técnica não é motivo para parar.
