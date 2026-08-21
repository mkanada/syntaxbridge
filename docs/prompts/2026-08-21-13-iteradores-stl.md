# Tarefa 13 — Iteradores STL: traduzir o idioma inteiro, não o iterador isolado

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
**F10**. Este prompt é autocontido.

**Faça a tarefa 07 antes desta**, se ainda não tiver sido feita: ela cria a
tabela de adaptadores de função livre da stdlib, que é onde `std::find_if` e
companhia vão morar.

## A causa raiz

`lower::cpp::lower_stdlib_method_call` (`crates/server/src/lower/cpp.rs`, por
volta da linha 6870) mapeia métodos de contêiner para o equivalente Dart
(`vector::size` → `.length`, `vector::empty` → `.isEmpty`, e dezenas de
outros). Mas `begin()`/`end()` não têm equivalente Dart isolado — Dart não
expõe iteradores posicionais sobre `List<T>` — então eles caem em bailout, e o
tipo do iterador (`__gnu_cxx::__normal_iterator`, o nome interno da libstdc++)
vaza como se fosse um tipo do domínio.

A contagem de diagnósticos diretos é pequena, mas cada ocorrência derruba um
idioma inteiro.

## A evidência

`.diagnosis/dart-package/lib/alignfunctor.dart:662` — uma única linha C++
(`std::find_if(verses.begin(), verses.end(), ObjectComparison(VERSE))`) produz
quatro falhas encadeadas:

```dart
__normal_iterator verseIterator = __normal_iterator(find_if(
  _syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…: unsupported std::vector::begin call'),
  _syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…: unsupported std::vector::end call'),
  ObjectComparison(ClassId.VERSE)));
if (!(_syntaxBridgeUnsupported<SyntaxBridgeOpaque>('…: unsupported free operator overload: operator=='))) {
  Verse? verse = verseIterator.unsupportedOperator();
  //                          ^ a desreferência do iterador (operator*)
```

Em Dart, o idioma inteiro é uma linha:

```dart
final verse = verses.where((v) => …).firstOrNull;
```

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791
diagnósticos) atribui a esta família **44** ocorrências diretas:

| `code` | n | detalhe |
| --- | ---: | --- |
| `undefined_class` | 28 | `Undefined class '__normal_iterator'` — `zip_file.dart`, `pugixml.dart`, `alignfunctor.dart`, `beamspan.dart`, `convertfunctor.dart` |
| `for_in_of_invalid_type` | 8 | `The type 'xpath_node_set' used in the 'for' loop must implement 'Iterable'` |
| `undefined_method` | 7 | métodos sobre o iterador |
| `non_type_as_type_argument` | 1 | `__normal_iterator` como argumento genérico (`beamspan.dart:145`) |

Mas o alcance real é maior: cada uma dessas linhas arrasta 2-4 bailouts
(`unsupported std::vector::begin call`, `unsupported std::vector::end call`,
`unsupported free operator overload: operator==`, `unsupportedOperator`) que
aparecem na métrica de stub de `.diagnosis/verovio-6.2.0.md` e que **não**
contam nos 44.

Outro campo do mesmo tipo: `.diagnosis/dart-package/lib/convertfunctor.dart:354`
→ `late __normal_iterator _m_currentMeasure;` — um iterador guardado como
**campo**, com vida longa, usado em 5 métodos diferentes.

## Onde mexer

- `crates/server/src/lower/cpp.rs`:
  - `lower_stdlib_method_call` (~6870) — onde `begin`/`end` hoje caem em
    bailout.
  - A tabela de funções livres da stdlib criada pela tarefa 07 — onde
    `find_if`, `find`, `sort`, `count_if`, `remove_if`, `for_each` entram.
  - `lower_type` (~1567) — o ponto onde o tipo do iterador é resolvido; ele já
    tem um braço para templates da stdlib (`vector`, `list`, `deque`, `array`,
    `initializer_list`, `multiset`, `stack`).

A direção é tratar o **idioma**, não a peça:

1. **Reconhecer o padrão completo.** `alg(c.begin(), c.end(), pred)` é uma
   única unidade de tradução. Reconheça-o no lowering da chamada ao algoritmo,
   consumindo os `begin`/`end` como delimitadores, e não emita bailout para
   eles.

   | C++ | Dart |
   | --- | --- |
   | `find_if(b, e, p)` + comparação com `end()` | `c.where(p).firstOrNull` (ou `indexWhere`, se a posição importar) |
   | `find(b, e, v)` | `c.indexOf(v)` / `c.contains(v)` |
   | `sort(b, e)` / `sort(b, e, cmp)` | `c.sort()` / `c.sort(cmp)` |
   | `count_if(b, e, p)` | `c.where(p).length` |
   | `for_each(b, e, f)` | `c.forEach(f)` |
   | `remove_if` + `erase` | `c.removeWhere(p)` |

   Repare que `find_if` seguido de `it != c.end()` e `*it` é um idioma de
   **três** statements em C++ e um em Dart. Se o reconhecimento não alcançar os
   três, uma tradução parcial é pior que nenhuma — nesse caso, bailout de
   statement honesto.

2. **Iterador de vida longa.** Quando o iterador é guardado (campo
   `_m_currentMeasure`, ou uma local que sobrevive além do idioma), não há
   idioma para reconhecer. Aí a resposta certa, segundo `AGENTS.md`, é um
   **adaptador nomeado**: um cursor sobre `List<T>` declarado no arquivo de
   suporte (`syntax_bridge_support.dart`), com `current`, `moveNext`, `isEnd` —
   e `__normal_iterator` nunca sobrevive como nome de tipo no Dart emitido.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro, idioma completo:
   ```cpp
   #include <algorithm>
   #include <vector>
   int achar(const std::vector<int> &v, int alvo) {
       auto it = std::find(v.begin(), v.end(), alvo);
       if (it != v.end()) return *it;
       return -1;
   }
   ```
   Verifique que o Dart emitido não contém `__normal_iterator` nem bailout de
   `begin`/`end`. Veja `crates/server/tests/lower_cpp.rs` para o padrão de
   fixture.
2. Teste do iterador de vida longa: um iterador guardado num campo. Verifique
   que o tipo emitido é o adaptador nomeado, não `__normal_iterator`.
3. Implemente até passar.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `__normal_iterator` não aparece no pacote emitido. Verificação direta:
  `grep -rc "__normal_iterator" .diagnosis/dart-package/lib/` → zero.
- `undefined_class` → queda de 28 (o resto são tipos de libc, tarefa 07).
- `non_type_as_type_argument` → queda de 1 (o resto é `__va_list_tag`, tarefa 07).
- `for_in_of_invalid_type` → queda de 8.
- **Métrica que importa mais que as contagens acima:** a linha
  "Linhas stub (expressão)" em `.diagnosis/verovio-6.2.0.md` deve **cair**.
  Esta é a única tarefa do lote cujo ganho principal é converter bailout em
  tradução real, não fazer erro sumir. Registre o antes/depois no resumo.
- Nenhum `code` novo.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso previsível: `find_if` que devolve um
iterador comparado com `end()` **e** desreferenciado tem tradução Dart limpa
(`firstOrNull` + `!= null`); mas se o código original *modifica* através do
iterador, ou faz aritmética com ele (`it + 2`, `it - begin()`), não há
equivalente e a escolha entre adaptador-cursor e bailout muda a forma do
produto. Pergunte se o corpus real tiver casos assim em volume.

Dificuldade técnica não é motivo para parar.
