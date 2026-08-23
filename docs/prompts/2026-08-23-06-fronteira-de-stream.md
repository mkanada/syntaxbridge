# Tarefa 06 — Fronteira nomeada para `std::ostream` / `std::istream`

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório; `dynamic` proibido; quando não houver
equivalente direto em Dart, a resposta é **uma fronteira/adaptador nomeado e
explícito, nunca um apagamento do tipo**).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/estado-da-transpilacao-verovio-6.2.md`,
família **T6**. Este prompt é autocontido.

**Esta é a maior família do backlog em volume de bailout: ~10.000 de 28.474.**
Ela não elimina erros do `dart analyze` — ela converte bailout em tradução
real, que é a métrica que importa a partir daqui.

## A causa raiz

`lower::cpp` reconhece exatamente **dois** casos de stream:

| função | linha | o que reconhece |
| --- | ---: | --- |
| `lower_ostream_insertion_chain` | `crates/server/src/lower/cpp.rs:7898` | uma cadeia `std::cout << a << b << std::endl` que **começa nos globais** `std::cout`/`std::cerr` |
| `lower_stringstream_insertion_chain` | `crates/server/src/lower/cpp.rs:8038` | uma cadeia que termina numa **variável local** `std::stringstream` |

O reconhecimento é por *cadeia inteira*, e o comentário de
`known_ostream_global` (`cpp.rs:7826-7838`) declara o escopo explicitamente:
"este bridge só nomeia os dois streams *globais*, nunca um substituto para um
stream arbitrário".

Todo o resto — e o Verovio é feito do resto — não tem tipo Dart nenhum.
`std::ostream` cai em `Type::Unsupported`, o parâmetro vira
`SyntaxBridgeOpaque`, e o método inteiro vira bailout.

## A evidência

Do snapshot `.diagnosis/verovio-6.2.0.json` (commit `32dd1df`):

**Tipos sem mapeamento** (total da categoria: 10.764):

| n | spelling |
| ---: | --- |
| 3.934 | `std::basic_ostream (spelling: basic_ostream<char, std::char_traits<char>>)` |
| 1.946 | `std::basic_ostream (spelling: std::basic_ostream<char>)` |
| 451 | `std::basic_ostream (spelling: basic_ostream<char>)` |
| 189 + 121 + 59 | as três grafias de `std::basic_istream` |
| 32 | `std::basic_ios (spelling: const std::basic_ios<char>)` |

**Expressões sem lowering** (total da categoria: 17.003):

| n | causa |
| ---: | --- |
| 2.630 | `unsupported free operator overload: operator<<` |
| 1.290 | `unsupported std::basic_ostream::operator<< call` |
| 227 | `unsupported implicit conversion from Str to Unsupported("std::basic_ostream …")` |

No C++ de origem há **845 linhas** com `ostream &` e **370** com
`ofstream`/`ifstream`/`stringstream`. As três formas que faltam:

1. **`std::ostream&` como parâmetro** — o idioma canônico do humlib:
   ```cpp
   std::ostream& operator<<(std::ostream& out, const HumNum& number);   // humlib.h
   void HumdrumLine::printXml(std::ostream& out, int level);
   ```
2. **`std::ofstream` / `std::ifstream`** para arquivo.
3. **stream guardado em campo** ou devolvido de função.

## O que fazer

O `AGENTS.md` já diz qual é a resposta: uma **fronteira externa explicitamente
modelada**. Um `std::ostream` não tem equivalente direto em Dart, então ele
precisa de um adaptador nomeado, não de apagamento.

### 1. O adaptador, em `syntax_bridge_support.dart`

O arquivo de suporte já existe e já hospeda `SyntaxBridgePair`,
`SyntaxBridgeNativeHandle`, `SyntaxBridgeOpaque` e `SyntaxBridgeListCursor`
(veja `SUPPORT_FILE_NAME`, `crates/server/src/emit/dart.rs:52`). Acrescente:

```dart
abstract class SyntaxBridgeOutputStream {
  void write(String text);
  void writeln([String text = '']);
  void flush();
}

class SyntaxBridgeStringOutputStream extends SyntaxBridgeOutputStream { … }  // StringBuffer
class SyntaxBridgeStdoutStream extends SyntaxBridgeOutputStream { … }        // print / stdout
class SyntaxBridgeStderrStream extends SyntaxBridgeOutputStream { … }        // stderr
class SyntaxBridgeFileOutputStream extends SyntaxBridgeOutputStream { … }    // dart:io File
```

E o par de entrada:

```dart
abstract class SyntaxBridgeInputStream {
  String? readLine();
  int readByte();     // -1 no fim, como o C++
  bool get eof;
}
```

Regras que a fronteira precisa respeitar para ser honesta:

- **Nada de `dynamic`.** Cada método tem tipo concreto.
- **Nada de mock silencioso.** Se uma operação de stream não tiver tradução
  (manipuladores como `std::setw`, `std::hex`, `std::setprecision`), ela vira
  bailout **na própria operação**, não no método inteiro — e o resto do método
  continua traduzido.
- `dart:io` só é importado nos arquivos que realmente usam stdout/arquivo — o
  emissor já tem esse padrão de import opt-in (`used_utf8_encode`,
  `emit/dart.rs:471`).

### 2. O mapeamento de tipo

Em `lower::cpp::lower_type`, reconhecer os templates da stdlib
`basic_ostream`, `basic_ostringstream`, `basic_ofstream`, `basic_iostream` como
um `Type::Record` do adaptador de saída, e `basic_istream`,
`basic_istringstream`, `basic_ifstream` como o de entrada. Há um precedente
direto: `Type::Str` para `basic_string` (`cpp.rs:2157`) e o
`SyntaxBridgeNativeHandle`, que já é um `Type::Record` com `usr` sintético
(`"syntax-bridge:native-handle"`). Use o mesmo mecanismo, com `usr`s
sintéticos próprios.

`stdlib_template_name` (`cpp.rs:2509`) já é a função que responde "qual template
da stdlib é este tipo" — as três grafias diferentes de `basic_ostream` que
aparecem na tabela de bailout são a mesma coisa vista por caminhos de
`typedef` diferentes, e essa função é onde elas se unificam.

### 3. O `operator<<`

Três formas, todas para `out.write(...)`:

| C++ | Dart |
| --- | --- |
| `out << x` (membro de `basic_ostream`) | `out.write(<x como String>)` |
| `out << x` resolvendo para o `operator<<` **livre** da stdlib | idem |
| `out << x` resolvendo para um `operator<<` **livre do projeto** (`operator<<(ostream&, const HumNum&)`) | a função livre passa a receber o adaptador e a ser chamada normalmente |

A conversão de `x` para `String` já existe em
`lower_stringstream_insertion_chain` (`cpp.rs:8080-8098`): literal passa
direto, `Type::Str` passa direto, `Int`/`Double` ganham `.toString()`. Extraia
essa lógica para uma função compartilhada em vez de duplicá-la uma terceira
vez — e estenda-a: um `Record` com `operator<<` livre próprio deve chamar essa
função livre, não virar bailout.

`std::endl` vira `writeln()`; `std::flush` vira `flush()`.

### 4. Os dois casos que já funcionam

`std::cout`/`std::cerr` hoje viram `print(...)`/`stderr.writeln(...)` e uma
`std::stringstream` local vira concatenação de `String`. **Não quebre isso sem
necessidade** — mas se ficar mais simples tratá-los como instâncias do novo
adaptador (`SyntaxBridgeStdoutStream`, `SyntaxBridgeStringOutputStream`), é
melhor: uma forma só em vez de três. Se você unificar, os goldens de
`examples/` mudam; revise o diff antes de `just examples-bless`.

## Escopo — o que **não** fazer

Esta família é grande o bastante para virar poço sem fundo. Fica **fora**:

- manipuladores de formatação (`std::setw`, `std::hex`, `std::setprecision`,
  `std::fixed`) — bailout na operação, com a mensagem dizendo qual manipulador;
- `std::ios_base` flags, `rdbuf()`, `tellp()`/`seekp()`;
- `std::wostream` e qualquer coisa que não seja `char`;
- `operator>>` de entrada formatada (`in >> x`) — só `getline`/`read` na
  primeira passada, e bailout explícito no resto.

Registre no resumo quantos bailouts sobraram por cada item excluído.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** — o idioma do humlib, que é o alvo:

   ```cpp
   #include <ostream>
   #include <sstream>
   #include <string>
   class Num {
   public:
       int valor = 3;
   };
   std::ostream &operator<<(std::ostream &out, const Num &n) {
       out << n.valor;
       return out;
   }
   std::string texto(const Num &n) {
       std::ostringstream ss;
       ss << "n=" << n;
       return ss.str();
   }
   ```

   Verifique que nem `texto` nem `operator<<` viram bailout, e que o Dart
   emitido usa o adaptador.

2. **Teste de parâmetro**: um método `void imprime(std::ostream &out) const;`
   chamado com um `ostringstream` e com `std::cout`.

3. **Teste comportamental**: acrescente o caso a `examples/E05-biblioteca-padrao/`
   (ou crie um degrau novo se ele não couber lá) com `oracle/cases.json`, para
   que a saída seja **executada** e comparada com a do C++. Sem isso, esta
   tarefa não tem prova.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- **A métrica principal são as contagens de bailout de
  `.diagnosis/verovio-6.2.0.md`**, não o `dart analyze`:
  - "Tipo C++ sem mapeamento": **10.764 → abaixo de 4.500** (os ~6.700 de
    `basic_ostream`/`basic_istream` somem);
  - "Expressão sem lowering": **17.003 → abaixo de 13.000**.
- `grep -rc "basic_ostream" .diagnosis/dart-package/lib/` → **zero**.
- É **esperado** que os erros do `dart analyze` **subam**: código que antes era
  um bailout passa a ser Dart real e a ser tipado. Registre o antes/depois e
  classifique os erros novos — se algum grupo novo passar de ~200 ocorrências,
  investigue antes de declarar a tarefa concluída.
- `examples/` inteiro continua passando, incluindo os oráculos.

## Quando parar e perguntar

Por decisão de **produto**, e aqui há uma real: `std::ofstream` escreve em
arquivo, e o pacote Dart gerado precisaria de `dart:io` — o que o impede de
rodar na web. Se o produto pretende gerar pacotes Dart web-compatíveis, a
resposta certa é uma interface abstrata com implementação injetável, não
`dart:io` direto. Pergunte, com a contagem de quantos pontos do Verovio
dependem de arquivo.

Dificuldade técnica não é motivo para parar.
