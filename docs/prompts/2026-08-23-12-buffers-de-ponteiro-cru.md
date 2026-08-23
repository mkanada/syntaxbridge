# Tarefa 12 — Ponteiro cru como buffer: `pugixml.cpp` e `zip_file.hpp`

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
família **T12**. Leia também `docs/plans/verovio-6.2-pointer-types.md` e
`docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`, que registram as decisões
de mapeamento de ponteiro já tomadas. Este prompt é autocontido no resto.

**Esta é a família com maior risco de virar poço sem fundo.** O escopo abaixo é
parte do trabalho, não uma sugestão: respeite-o e registre o que sobrou.

## A causa raiz

Dois arquivos do Verovio são C, não C++: `src/pugi/pugixml.cpp` (o parser XML) e
`include/zip/zip_file.hpp` (miniz, a descompressão de MXL). Eles usam ponteiro
cru como **cursor sobre um buffer**: aritmética (`p + 4`, `++p`), indexação
(`p[i]`), escrita indexada (`p[i] = v`), comparação (`p < end`) e endereço
(`&x`).

O bridge mapeia ponteiro-para-escalar-conhecido para `Type::Bytes`
(`Uint8List`) ou para `Nullable(T)` (`lower_type`,
`crates/server/src/lower/cpp.rs:1900-1925`), o que está certo para "um ponteiro
é nulo ou um valor" — mas nenhum desses mapeamentos sustenta um **cursor com
deslocamento**. `Uint8List` não tem `+`; `T?` não tem `[i]`.

## A evidência

`dart analyze` (`.diagnosis/verovio-6.2.0.analyze.json`, commit `32dd1df`):

| `code` | n | onde |
| --- | ---: | --- |
| `unchecked_use_of_nullable_value` | 372 | `pugixml.dart` 279, `zip_file.dart` 85 |

Das 279 de `pugixml.dart`: 157 são `The method 'X' can't be unconditionally
invoked because the receiver can be 'null'` e 119 são o mesmo com um operador
(`<`, `>`, `<=`, `>=`, `+`). `pugixml.dart:174-186` é a comparação de dois
ponteiros; `pugixml.dart:501` é `root!.first_child + 0`.

Bailouts do snapshot:

| n | causa |
| ---: | --- |
| 332 | `array subscript receiver is not a lowered Dart collection` |
| 303 | `assignment target is not a simple local variable or a field (index assignment not supported yet)` |
| 129 | `address-of requires a representable nullable reference` |
| 18 | `compound assignment target is not a simple local variable or a field` |
| 11 | `assignment target is not representable as a Dart assignment target` |

Tipos sem mapeamento, da mesma origem: `FILE *` (100), `int *` (62),
`char **` (51), `const mz_uint16 *` (51), `mz_uint64 *` (53), `size_t *` (42),
`uint32_t *` (31), `uint16_t *` (29), `const unsigned char *` (28), `struct tm *`
(58), `struct stat`, `timeval`.

`.diagnosis/dart-package/lib/zip_file.dart:676-686` mostra o formato:

```dart
mz_crc32 = mz_crc32 >> 8 ^ s_crc_table[(mz_crc32 ^ _syntaxBridgeUnsupported<int>('…: array subscript receiver is not a lowered Dart collection')) & 255];
pByte_buf = pByte_buf + 4;
…
++pByte_buf;
```

O C++ (`include/zip/zip_file.hpp:1607`) é `*pByte_buf++`.

## O que fazer

### 1. O adaptador de buffer, em `syntax_bridge_support.dart`

Um cursor sobre bytes, com deslocamento, na mesma linha do
`SyntaxBridgeListCursor` que a tarefa 13 do lote anterior criou:

```dart
final class SyntaxBridgeByteCursor {
  SyntaxBridgeByteCursor(this._bytes, [this._offset = 0]);

  final Uint8List _bytes;
  int _offset;

  int operator [](int i) => _bytes[_offset + i];
  void operator []=(int i, int v) { _bytes[_offset + i] = v; }
  int get value => _bytes[_offset];
  set value(int v) { _bytes[_offset] = v; }

  SyntaxBridgeByteCursor operator +(int n) => SyntaxBridgeByteCursor(_bytes, _offset + n);
  SyntaxBridgeByteCursor operator -(int n) => SyntaxBridgeByteCursor(_bytes, _offset - n);
  int distanceTo(SyntaxBridgeByteCursor other) => other._offset - _offset;
  bool operator <(SyntaxBridgeByteCursor other) => _offset < other._offset;
  // …>, <=, >=, ==
}
```

Um cursor genérico equivalente sobre `List<T>` cobre `int *`, `uint32_t *` e
companhia — mas **só crie o genérico se o corpus exigir**; meça antes.

### 2. Onde o adaptador entra

Um ponteiro-para-escalar só vira cursor quando o código de fato o usa como
buffer. O critério observável, na ordem em que deve ser testado:

1. o ponteiro é alvo de aritmética (`p + n`, `p - n`, `++p`, `p += n`); ou
2. o ponteiro é indexado (`p[i]`), com `i` que não é constante `0`; ou
3. o ponteiro é comparado com outro ponteiro por `<`/`>`/`<=`/`>=`.

Se nenhuma das três acontecer, o mapeamento atual (`T?`) está certo e deve
ficar — não troque tudo por cursor.

Esse critério é **por declaração** (variável, parâmetro ou campo), não por uso:
uma vez que uma variável é cursor, ela é cursor em todos os seus usos, senão o
tipo muda no meio do corpo. `pointer_catalog.rs` já é o lugar do produto que
raciocina sobre ponteiros por declaração — leia-o antes de criar um mecanismo
paralelo.

### 3. As três operações que hoje bailoutam

| bailout | com o cursor |
| --- | --- |
| `array subscript receiver is not a lowered Dart collection` | `p[i]` |
| `assignment target is not a simple local variable or a field (index assignment…)` | `p[i] = v` |
| `address-of requires a representable nullable reference` | ver item 4 |

### 4. `&x` (endereço)

129 bailouts. `&arr[0]` e `&vetor[0]` são "cursor a partir daquela posição" e
cabem no adaptador. `&escalarLocal`, passado a uma função que escreve nele, é
um **out-param** e já tem ponte própria (`apply_out_param_bridge`) — verifique
por que ela não está alcançando esses casos antes de inventar mecanismo novo.
`&funcao` é ponteiro de função e fica **fora** desta tarefa.

### 5. Escopo — o que fica de fora

- `FILE *`, `struct stat`, `struct tm`, `timeval` — são **fronteira externa**,
  não buffer. O produto já tem o conceito
  (`docs/plans/lista-de-externos.md`, `crates/server/src/externals.rs`);
  garanta que eles são reconhecidos como externos e emitidos como fronteira
  nomeada, e **não** tente traduzi-los.
- `char **` (`argv`) — fica fora; a tarefa 15.9 do lote anterior já reescreveu
  o `main`.
- `reinterpret_cast` entre tipos de ponteiro de largura diferente
  (`mz_uint16 *` sobre um buffer de `mz_uint8`) — bailout explícito, com a
  mensagem dizendo as duas larguras. Traduzir isso corretamente exige
  `ByteData`, e isso é uma tarefa própria.
- alocação (`malloc`/`free`/`new[]`/`delete[]`) — fronteira externa.

Registre no resumo quantos bailouts sobraram por item excluído.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro**, o idioma exato do miniz:

   ```cpp
   #include <cstdint>
   unsigned soma(const unsigned char *buf, int len) {
       unsigned total = 0;
       const unsigned char *p = buf;
       while (len >= 4) {
           total += p[0] + p[1] + p[2] + p[3];
           p = p + 4;
           len -= 4;
       }
       while (len--) { total += *p++; }
       return total;
   }
   ```

   Nenhum bailout; `dart analyze` sem erro.

2. **Teste de escrita indexada**:

   ```cpp
   void zera(unsigned char *buf, int len) { for (int i = 0; i < len; i++) buf[i] = 0; }
   ```

3. **Teste comportamental.** `examples/E10-ponteiros-union-out-params/` já
   existe e tem oráculo — acrescente os dois casos acima lá. Aritmética de
   ponteiro é onde "compila mas faz outra coisa" é mais provável.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `unchecked_use_of_nullable_value`: **372 → abaixo de 40**.
- Bailouts `array subscript receiver is not a lowered Dart collection`:
  **332 → 0**.
- Bailouts `assignment target is not a simple local variable or a field (index
  assignment not supported yet)`: **303 → 0**.
- Bailouts `address-of requires a representable nullable reference`: cai pelo
  menos à metade; o resto tem de estar explicado no resumo.
- "Tipo C++ sem mapeamento": queda de pelo menos **300**.
- Nenhum `code` novo. Erros podem subir em `zip_file.dart`/`pugixml.dart` —
  registre e classifique.

## Quando parar e perguntar

Por decisão de **produto**, e há uma grande: **estes dois arquivos precisam ser
transpilados?** `pugixml` e `miniz` são bibliotecas de terceiros embutidas no
Verovio, com equivalentes Dart maduros (`package:xml`, `package:archive`).
Tratá-los como **fronteira externa** — o mecanismo que o produto já tem em
`docs/plans/lista-de-externos.md` — elimina ~1.000 bailouts e ~400 erros sem
escrever uma linha de tradução de ponteiro, e provavelmente produz um resultado
melhor.

**Levante essa pergunta antes de começar a implementar**, com os números. Se a
resposta for "sim, transpile", siga o plano acima. Se for "não, são externos",
esta tarefa vira outra, muito menor, e o adaptador de buffer só precisa cobrir o
que sobrar fora desses dois arquivos.

Dificuldade técnica não é motivo para parar.
