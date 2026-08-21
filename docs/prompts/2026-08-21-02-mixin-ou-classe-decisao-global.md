# Tarefa 02 — Decidir `class` vs. `mixin` numa fase com visão do projeto inteiro

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido, mapeamento de tipos é o
objetivo central do produto).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F4**. Este prompt é autocontido.

**Execute a tarefa 01 (`2026-08-21-01-uniao-de-registros-no-merge.md`) antes
desta**, se ela ainda não tiver sido feita: ela muda drasticamente o conteúdo
dos registros e re-mede a linha de base.

## A causa raiz

C++ tem herança múltipla; Dart não. O pipeline resolve isso emitindo as bases
como `mixin` e aplicando-as com `with`. A decisão de emitir um registro como
`mixin` em vez de `class` já é global: `emit::dart::mixin_usrs`
(`crates/server/src/emit/dart.rs`, por volta da linha 225) varre o `Module`
inteiro e coleta todo `usr` usado como mixin, transitivamente, e
`emit::dart::emit_record` usa esse conjunto para escolher a palavra-chave.

**Mas a decisão para na declaração.** Os *usos* daquele mesmo registro não são
revistos:

1. Um registro que virou `mixin` continua sendo alvo de `extends` quando algum
   outro registro o tem como `Record::base_class` (base única). Dart rejeita:
   `Classes can only extend other classes.`
2. Um registro que virou `mixin` perde o construtor posicional sintético que
   todo registro com campos ganha (Dart proíbe construtor não-default numa
   classe usada com `with`), mas os *call sites* que constroem esse registro
   continuam passando argumentos.

Em `.diagnosis/dart-package/lib/atts_shared.dart`:

```dart
 7: mixin AttAccidLog on Att { … }
19: class InstAccidLog extends AttAccidLog { }   // ← extends_non_class
…
42: class AttAnnotLog extends Att { … }          // ← Att também é mixin
```

Em `.diagnosis/dart-package/lib/accid.dart:52`, dentro de `Clone()`:

```dart
return Accid(this._m_drawingUnison, this._m_alignedWithSameLayer, this._m_floatingObject);
// ← Too many positional arguments: 0 expected, but 3 found
```

Um efeito colateral da mesma causa: em `.diagnosis/dart-package/lib/fig.dart:26`,
`return this is AreaPosInterface ? this : null;` dispara `unnecessary_type_check`
— a classe já aplica `AreaPosInterface` como mixin, então o teste é sempre
verdadeiro, enquanto o `dynamic_cast` C++ correspondente podia falhar.

## A evidência

`dart analyze` sobre o pacote emitido do Verovio 6.2.0
(`.diagnosis/verovio-6.2.0.analyze.json`, 24.791 diagnósticos):

| `code` | n | arquivos | concentração |
| --- | ---: | ---: | --- |
| `extends_non_class` | 361 | 65 | `atts_shared.dart` (149), `atts_visual.dart` (39), `atts_cmn.dart` (29) |
| `extra_positional_arguments` | 481 | 65 | `iomei.dart` (75), `atts_shared.dart` (51), `editortoolkit_neume.dart` (43) |
| `unnecessary_type_check` | 4 | 2 | `fig.dart`, `rend.dart` |

Nem todos os 481 `extra_positional_arguments` são desta família (alguns são
variádicos C++, tratados na tarefa 15 — `LogError('%s', str)`), mas a maioria é.

## Onde mexer

A proposta é uma **fase nova entre o `Module` completo e a emissão** — não uma
correção local dentro de `emit_record`. Ela existe porque a decisão de tradução
de um símbolo depende de como ele é usado em *outros* pontos do projeto, não da
sua própria declaração.

- **Que decisão ela resolve:** a forma Dart de cada registro — `class`,
  `mixin`, ou `abstract class`.
- **Que dado ela produz:** um mapa `usr → forma`, e a reescrita do IR para que
  todos os consumidores concordem — cada `Record::base_class` que aponta para um
  registro-mixin migra para `Record::mixins`, e cada construção de um
  registro-mixin passa a usar a forma que aquela decisão permite.
- **Onde é consumido:** `emit::dart::emit_record` deixa de derivar a forma
  sozinho (hoje via `mixin_usrs` + `is_mixin`) e passa a lê-la do IR. Isso
  elimina por construção a possibilidade de declaração e uso discordarem.

Arquivos prováveis:

- `crates/server/src/function_catalog.rs` — é onde as outras passadas globais
  sobre o `Module` já moram (`apply_overload_renames`,
  `apply_record_name_disambiguation`, `apply_raii_scope_guards`,
  `apply_reserved_word_renames`). A fase nova encaixa na mesma sequência, em
  `finish_function_catalog`.
- `crates/server/src/ir/mod.rs` — provavelmente um campo novo em `ir::Record`
  para a forma decidida. Os doc comments de `Record::base_class` e
  `Record::mixins` descrevem a invariante atual ("nunca os dois ao mesmo
  tempo"); mantenha-a ou documente a mudança.
- `crates/server/src/emit/dart.rs` — `mixin_usrs`, `expand_mixin_chain`,
  `emit_record`, `emit_module`.

Um registro que é `mixin` mas precisa ser instanciado não pode simplesmente
perder o construtor. Duas saídas viáveis, e o "como" é seu: uma fábrica estática
(`static X create(...)`) que o call site passa a chamar; ou uma classe concreta
irmã (`class XImpl with X`) usada nas instanciações. Escolha uma e aplique-a de
forma consistente — **não** deixe o call site sem alvo.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro. O fixture mínimo: três classes C++ onde `A` é base
   única de `B` (viraria `extends`) e ao mesmo tempo uma das duas bases de `C`
   (força `A` a virar `mixin`), com `B` sendo instanciada em algum lugar.
   Verifique que o Dart emitido não usa `extends A` e que a construção de `B`
   compila. `crates/server/tests/lower_cpp.rs` e os testes de emissão existentes
   mostram o padrão.
2. Implemente até passar.
3. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `extends_non_class` → **zero**. É um erro puramente estrutural: se sobrar
  algum, a fase não está cobrindo um caminho.
- `extra_positional_arguments` → queda forte. O resíduo esperado são os
  variádicos C++ (`LogError`, `LogWarning`, `LogDebug` — cerca de 100
  ocorrências), que a tarefa 15 trata. Se o resíduo for muito maior, investigue.
- `unnecessary_type_check` → zero ou substituído por um bailout honesto (ver
  abaixo).
- Nenhum `code` novo. Em particular, `mixin_of_non_class`,
  `mixin_application_not_implemented_interface` e
  `mixin_inherits_from_not_object` são os erros que uma decisão inconsistente na
  direção oposta produziria — se qualquer um aparecer, a fase está errada.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso real que provavelmente vai aparecer: um
registro que precisa ser mixin (herança múltipla) **e** instanciado
diretamente. Fábrica estática e classe-irmã concreta são as duas saídas, elas
mudam a forma do Dart gerado de maneira observável, e a escolha vale para o
produto inteiro — pergunte antes de fixar uma.

Outro: em C++, `x is AreaPosInterface` sobre uma classe que *declara* aquela
interface é sempre verdadeiro em Dart, mas o `dynamic_cast` original podia
falhar em tempo de execução se o objeto fosse de outro tipo dinâmico. Se a
semântica não puder ser preservada, isso precisa ser um bailout explícito, não
um `true` silencioso — mas se você achar uma tradução que preserve, melhor.
Pergunte se as duas parecerem defensáveis.

Dificuldade técnica não é motivo para parar.
