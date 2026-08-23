# Tarefa 09 — Iteradores de `std::list`/`std::map`, reversos, e `std::vector<bool>`

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório; `dynamic` proibido; quando não houver
equivalente direto em Dart, a resposta é uma fronteira/adaptador nomeado e
explícito, nunca um apagamento).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/estado-da-transpilacao-verovio-6.2.md`,
família **T9**. Este prompt é autocontido.

**Leia primeiro `docs/prompts/2026-08-21-13-iteradores-stl.md`**, a tarefa que
resolveu o mesmo problema para `std::vector`. Esta tarefa é a continuação dela
para os contêineres que ficaram de fora. Não refaça o que já está feito;
estenda.

## A causa raiz

A tarefa 13 do lote anterior ensinou o lowering a reconhecer o **idioma
inteiro** (`alg(c.begin(), c.end(), pred)`) sobre `std::vector`, e criou o
adaptador `SyntaxBridgeListCursor<T>` em `syntax_bridge_support.dart` para o
iterador de vida longa:

```dart
final class SyntaxBridgeListCursor<T> {
  SyntaxBridgeListCursor(this._items, [this._index = 0]);
  bool get isEnd => _index >= _items.length;
  T get current => _items[_index];
  void moveNext() { _index++; }
}
```

Ficaram de fora quatro coisas, e todas aparecem no Verovio em volume:

1. **`std::list`** — `_List_iterator`, com `operator*`, `operator->`,
   `operator++`. O humlib guarda `std::list<GridSlice*>` e navega com iterador.
2. **`std::map`/`std::set`** — `_Rb_tree_iterator`,
   `_Rb_tree_const_iterator`, com `operator->` sobre um `std::pair`.
3. **`std::reverse_iterator`** — `rbegin()`/`rend()`.
4. **`std::vector<bool>`** — que não é um `vector` de verdade: `operator[]`
   devolve `std::_Bit_reference`, um proxy.

Além disso, o `SyntaxBridgeListCursor` não tem **aritmética**: `it + 1`,
`it - v.begin()`, `it += n` aparecem e não têm operador.

## A evidência

Bailouts do snapshot `.diagnosis/verovio-6.2.0.json` (commit `32dd1df`):

| n | causa / tipo |
| ---: | --- |
| 232 | `unsupported std::_List_iterator::operator* call` |
| 122 | `unsupported std::_List_iterator::operator-> call` |
| 117 | `unsupported std::_List_iterator::operator++ call` |
| 109 | `unsupported std::_Rb_tree_iterator::operator-> call` |
| 109 | `unsupported std::list::begin call` |
| 353 / 296 | `std::vector::begin` / `std::vector::end` ainda em posições não reconhecidas |
| 80 / 75 | `std::basic_string::begin` / `back` |
| 67 | `unsupported std::reverse_iterator::operator* call` |
| 249 | tipo `std::_Bit_reference` / `const std::_Bit_reference` |
| 112 + 82 + 59 + 57 + 53 + 45 + 30 | tipo `std::_List_iterator<…>` nas suas várias grafias |
| 63 + 58 + 39 | tipo `std::_Rb_tree_iterator<…>` / `_Rb_tree_const_iterator<…>` |
| 45 + 28 | tipo `std::reverse_iterator<…>` |
| 93 | conversão `std::_Bit_reference` → `const std::_Bit_reference` |

Erros do `dart analyze`:

| `code` | n | forma |
| --- | ---: | --- |
| `undefined_operator` | 76 | `The operator '+' isn't defined for the type 'SyntaxBridgeListCursor<T>'` |
| `for_in_of_invalid_type` | 21 | `for` sobre tipo não iterável |
| `non_type_as_type_argument` | 20 | nome de iterador como argumento genérico |
| `undefined_method` | ~8 | `assignFrom` sobre `SyntaxBridgeListCursor` |

`.diagnosis/dart-package/lib/beamspan.dart:162` e
`drawinginterface.dart:161,173,175` mostram a aritmética faltando.

## O que fazer

### 1. Os três contêineres que faltam, no mesmo modelo do `vector`

`std::list` e `std::map`/`std::set` já têm representação Dart de contêiner
(`List<T>` e `Map<K, V>`); o que falta é o **iterador** sobre eles. Estenda o
mesmo reconhecimento de idioma que a tarefa 13 fez, e o mesmo adaptador de vida
longa:

| C++ | Dart |
| --- | --- |
| `for (auto it = l.begin(); it != l.end(); ++it) { … *it … }` | `for (final x in l) { … }` |
| `it->campo` | `x.campo` |
| `m.find(k)` comparado com `m.end()` | `m.containsKey(k)` / `m[k]` |
| `it->first` / `it->second` num `map` | `e.key` / `e.value` (a extensão `SyntaxBridgeMapEntryPair` já existe no arquivo de suporte) |
| `for (auto it = v.rbegin(); it != v.rend(); ++it)` | `for (final x in v.reversed) { … }` |

Quando o iterador **não** couber no idioma (guardado em campo, aritmética,
`erase` durante a travessia), a resposta é o `SyntaxBridgeListCursor` — que
precisa ganhar, no arquivo de suporte:

- `operator +(int)` / `operator -(int)` devolvendo um novo cursor;
- `int operator -(SyntaxBridgeListCursor<T> other)` (distância), a forma
  `it - v.begin()` que aparece 76 vezes;
- um cursor de mapa equivalente, se o corpus exigir — não invente antes de
  medir.

### 2. `std::vector<bool>`

`std::vector<bool>` é uma especialização empacotada em bits, e `v[i]` devolve
um proxy (`std::_Bit_reference`), não um `bool&`. Em Dart, `List<bool>` faz
exatamente o que o programador queria. A regra:

- o tipo `std::vector<bool>` mapeia para `List<bool>`;
- `_Bit_reference` e `const _Bit_reference` **nunca** aparecem como tipo: onde
  o `libclang` reporta um, o valor é o `bool` que ele representa;
- a conversão `_Bit_reference` → `const _Bit_reference` (93 bailouts) some por
  construção.

Verifique se `lower_type` já mapeia `std::vector<bool>` para `List(Bool)`; se
mapear, o trabalho é só apagar o proxy no lowering de expressão.

### 3. `std::basic_string::begin`/`back`

`s.begin()`/`s.end()` sobre `std::string` (80 bailouts) e `s.back()` (75) são
casos triviais dado que a string é indexada em bytes (veja a tarefa 04):
`back()` é o último byte; `begin()`/`end()` delimitam a travessia por bytes.
Faça-os junto — são o mesmo caminho de código.

### 4. Escopo

Fica de fora, com bailout **explícito e específico** (não genérico):

- `std::unordered_map` com iterador (se aparecer);
- iterador de `std::deque` com aritmética;
- inserção/remoção **através** do iterador durante a travessia
  (`l.erase(it++)`) — esse idioma não tem tradução direta e uma tradução
  parcial é pior que nenhuma;
- `std::multimap`/`equal_range`.

Registre no resumo quantos bailouts sobraram por item.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro**, um por contêiner:

   ```cpp
   #include <list>
   #include <map>
   #include <string>
   #include <vector>

   int somaLista(const std::list<int> &l) {
       int s = 0;
       for (auto it = l.begin(); it != l.end(); ++it) s += *it;
       return s;
   }

   int achaNoMapa(const std::map<std::string, int> &m, const std::string &k) {
       auto it = m.find(k);
       if (it != m.end()) return it->second;
       return -1;
   }

   int ultimoReverso(const std::vector<int> &v) {
       for (auto it = v.rbegin(); it != v.rend(); ++it) return *it;
       return 0;
   }

   bool primeiroBit(const std::vector<bool> &b) { return b[0]; }
   ```

   Para cada um: nenhum bailout, nenhum nome de iterador da libstdc++ no Dart
   emitido.

2. **Teste do cursor de vida longa com aritmética**: um iterador guardado numa
   variável e usado com `it + 1` e `it - v.begin()`.

3. **Teste comportamental.** Acrescente os quatro casos acima a
   `examples/E05-biblioteca-padrao/oracle/cases.json` (ou a um degrau novo).
   Travessia é exatamente o tipo de coisa que "compila e faz outra coisa".

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `grep -rcE "_List_iterator|_Rb_tree_iterator|reverse_iterator|_Bit_reference|__normal_iterator" .diagnosis/dart-package/lib/`
  → **zero**.
- "Tipo C++ sem mapeamento": queda de pelo menos **900**.
- "Expressão sem lowering": queda de pelo menos **1.500**.
- `undefined_operator`: **76 → abaixo de 10**.
- `for_in_of_invalid_type`: **21 → 0**.
- `non_type_as_type_argument`: **20 → 0**.
- Nenhum `code` novo.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível é o mesmo que a tarefa 13
levantou: um iterador que **modifica** através de si (`*it = x`, `l.erase(it)`)
ou faz aritmética densa não tem equivalente Dart limpo, e a escolha entre
adaptador-cursor com write-through e bailout muda a forma do produto. Pergunte
se o corpus real tiver casos assim em volume — meça antes.

Dificuldade técnica não é motivo para parar.
