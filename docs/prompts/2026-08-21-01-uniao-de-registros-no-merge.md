# Tarefa 01 — Fundir registros entre unidades de compilação por união, não "o primeiro vence"

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter (`client/flutter/`). Leia `AGENTS.md` na
raiz antes de começar — ele é normativo (TDD obrigatório, `dynamic` proibido,
bailouts precisam preservar o tipo estático e falhar explicitamente).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

O diagnóstico completo que originou esta tarefa está em
`docs/plans/dart-analyze-verovio-6.2.0.md`, família **F1**. Este prompt é
autocontido: você não precisa daquele documento para executar, mas ele tem os
dados de apoio.

## A causa raiz

`function_catalog::extract_function_catalog`
(`crates/server/src/function_catalog.rs`, por volta da linha 261) divide as
unidades de compilação do projeto em N pedaços, um por worker de
`std::thread::scope`. **Cada worker tem o seu próprio `VisitorState`**, com o
seu próprio `ir_records`.

Dentro de um worker o mecanismo está correto: quando o visitante encontra a
definição de um membro — inline no header ou out-of-line num `.cpp` — ele lowera
o membro e o anexa ao registro cujo `owning_class_usr` ele nomeia
(`function_catalog.rs`, por volta de 2515: `record.methods.push(method_ir)`).

O problema está no merge final, `finish_function_catalog`
(`function_catalog.rs`, por volta de 392):

```rust
for record in partial_ir_records {
    if ir_record_seen.insert(record.usr.clone()) {
        ir_records.push(record);
    }
}
```

**O primeiro partial que contiver aquele `usr` vence, e todos os outros são
descartados inteiros.**

Uma classe declarada num header compartilhado (`include/vrv/object.h`) é
lowered em *toda* unidade que inclui esse header, mas só ganha os métodos
out-of-line no worker que também processou o `.cpp` onde eles estão definidos
(`src/object.cpp`). Como `object.h` é incluído por quase todas as 298 unidades,
o partial que vence é o de alguma unidade que só viu o header — e a cópia rica
é jogada fora.

Repare que o mesmo arquivo **já resolve exatamente este problema** para funções
livres, algumas dezenas de linhas acima (`function_catalog.rs`, por volta de
372): lá, um protótipo aceito antes é *substituído* quando a definição chega
depois, usando `ir_function_is_prototype` como desempate. Registros não têm
nada equivalente.

## A evidência

Do pacote Dart emitido a partir do Verovio 6.2.0 real
(`.diagnosis/dart-package/lib/`, gerado por `just verovio-diagnosis`):

| Arquivo Dart | linhas | `.cpp` de origem | linhas |
| --- | ---: | --- | ---: |
| `doc.dart` | 167 | `src/doc.cpp` | 2482 |
| `object.dart` | 419 | `src/object.cpp` | 1684 |
| `note.dart` | 176 | `src/note.cpp` | 1027 |
| `accid.dart` | **483** | `src/accid.cpp` | 371 |

`accid.dart` é o caso que funcionou: tem `AdjustX`, `Reset`,
`AdjustToLedgerLines`, todos definidos out-of-line em `accid.cpp`. `doc.dart` e
`object.dart` só têm os métodos que o header define **com corpo dentro do
`class`**. `bool Object::AddChild(Object *child)` (`src/object.cpp:848`,
declarado como `virtual bool AddChild(Object *object);` em
`include/vrv/object.h:432`) **não existe em nenhum lugar do pacote emitido**, e
é chamado 409 vezes.

Outra confirmação: `xml_allocator`, declarado *dentro* de `src/pugi/pugixml.cpp`
(uma única unidade), saiu completo. `xml_node`, declarado em `pugixml.hpp` com
os métodos definidos out-of-line no `.cpp`, saiu com um campo e um construtor e
mais nada — e é o receptor de 1.399 `undefined_method`.

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`) reporta
24.791 diagnósticos. Esta causa raiz explica ~9.474 deles (38,2%):

| `code` | ocorrências atribuídas | por quê |
| --- | ---: | --- |
| `undefined_method` | 6.972 | o método out-of-line não existe |
| `override_on_non_overriding_member` | 750 | `@override` num método cuja base perdeu o dela |
| `not_enough_positional_arguments` | 583 | construtor real descartado; o emissor sintetizou um posicional a partir de todos os campos (`21 positional arguments expected by 'VrvMeasure.new'`) |
| `undefined_getter` | 471 | idem, para getters |
| `implicit_super_initializer_missing_arguments` | 319 | idem, para construtores de base |
| `undefined_operator` | 161 | `Fraction::operator+`, `HumNum::operator*` são out-of-line |
| `non_abstract_class_inherits_abstract_member` | 135 | `BoundingBox::GetDrawingX` é virtual pura e o override de `Object` está out-of-line |

Além disso, boa parte dos 1.759 `unused_field` é sintoma: o campo privado
sobrevive, mas os métodos que o liam sumiram.

## Onde mexer

- `crates/server/src/function_catalog.rs`, `finish_function_catalog` — o laço
  de merge de `partial_ir_records`.
- Possivelmente `crates/server/src/ir/mod.rs` se a fusão precisar de algum dado
  que hoje não está em `ir::Record`/`ir::Method`/`ir::Constructor` (todos já
  carregam `usr`, então provavelmente não precisa).
- `VisitorState.ir_member_seen` (mesmo arquivo) continua correto como está: ele
  evita lowering duplicado *dentro* de um worker. A deduplicação **entre**
  workers é o que precisa passar a existir, no merge.

Não prescrevo a implementação. A forma da solução é: fundir por `usr` de
registro, e dentro do registro fundir `methods`, `constructors`,
`static_fields` e `destructor` deduplicando por `usr` de membro, preferindo a
versão que tem corpo (`Method::body.is_some()`, `Constructor` com `body` não
vazio) quando dois partials trouxerem o mesmo membro. `fields`, `base_class`,
`mixins`, `namespace` e `origin` vêm da definição da classe e devem ser
idênticos entre partials — se não forem, isso é sinal de outro problema e
merece um diagnóstico explícito, não uma escolha silenciosa.

Atenção a `Constructor::constructor_index`: o doc comment dele já avisa que a
ordem de `Record::constructors` é ordem de visitação, não de declaração, e que
`emit::dart` ordena por esse campo. A união não pode quebrar essa invariante.

## Método

TDD, conforme `AGENTS.md`:

1. Escreva primeiro um teste que **falha**, em
   `crates/server/tests/` (veja `lower_cpp.rs` e os testes de
   `function_catalog` já existentes para o padrão de fixture). O teste mínimo
   que reproduz a causa: um projeto de duas unidades de compilação, um header
   com uma classe que declara um método sem corpo, e dois `.cpp` — um que só
   inclui o header, outro que define o método — forçando os dois a caírem em
   partials diferentes. Verifique que o `ir::Record` resultante contém o
   método. Se o teste depender de quantos workers a máquina tem, teste
   `finish_function_catalog` diretamente, montando os `FunctionCatalogPartial`
   à mão: isso é determinístico e é o que realmente está sendo corrigido.
2. Implemente até passar.
3. `just test` (ou `just test-host`, registrando no resumo).

## Critério de sucesso

Depois de `just verovio-diagnosis` (leva 5-6 min; se rodar dentro do Flatpak,
precisa de `just package-build` antes), comparando
`.diagnosis/verovio-6.2.0.analyze.json` com a linha de base abaixo:

| `code` | antes | esperado |
| --- | ---: | --- |
| `undefined_method` | 8309 | queda drástica (a maior parte dos 6.972 atribuídos) |
| `override_on_non_overriding_member` | 750 | perto de zero |
| `non_abstract_class_inherits_abstract_member` | 135 | perto de zero |
| `not_enough_positional_arguments` | 583 | queda forte |
| `implicit_super_initializer_missing_arguments` | 319 | queda forte |
| `undefined_operator` | 161 | queda forte |

Verificação direta, independente do agregado: `bool AddChild(...)` **precisa**
aparecer em `.diagnosis/dart-package/lib/object.dart`, e `doc.dart` precisa
crescer de 167 linhas para a ordem de grandeza de `doc.cpp`.

**Regressões são esperadas aqui e não são motivo para reverter.** Trazer de
volta milhares de métodos vai expor erros que estavam escondidos atrás da
ausência deles (chamadas a `std::` sem adaptador, downcasts perdidos, `!`
redundantes). Registre no resumo qual `code` subiu e quanto; as tarefas 02-15
tratam dessas famílias. O que **não** é aceitável é um `code` novo aparecer sem
explicação, ou o total de erros subir sem que a queda dos alvos acima tenha
acontecido.

Rode também `just check` e `just lint`.

## Quando parar e perguntar

Só se aparecer uma decisão de **produto**: duas soluções tecnicamente válidas,
mutuamente exclusivas, que mudam comportamento observável. Um caso plausível: se
dois partials trouxerem o mesmo membro (mesmo `usr`) com corpos **diferentes** —
o que acontece quando macros expandem de forma distinta em unidades diferentes —
escolher um deles é arbitrário. Se isso ocorrer no corpus real, pergunte em vez
de escolher em silêncio.

Dificuldade técnica não é motivo para parar.
