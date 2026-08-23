# Tarefa 07 — Operadores de conversão e herança de tipo de biblioteca

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
família **T7**. Este prompt é autocontido.

São **duas metades** com a mesma raiz — "um objeto do projeto que se comporta
como um valor de outro tipo" — e ~3.800 bailouts somados. Faça as duas.

## Metade A — operadores de conversão (`operator T()`)

### A causa raiz

O pugixml usa o idioma *safe bool* em quatro classes
(`include/pugi/pugixml.hpp:395, 516, 812, 1301`):

```cpp
typedef void (*unspecified_bool_type)(xml_node***);
operator unspecified_bool_type() const;
```

Isso faz `if (element.attribute("func"))` compilar sem permitir
`int x = element;`. `lower::cpp` não lowera `CXCursor_ConversionFunction`, então
a conversão implícita que o `libclang` reporta no ponto de uso não tem para
onde ir e vira bailout — com a mensagem que aparece na tabela literalmente como

```
unsupported implicit conversion from Callback { return_type: Void,
params: [Unsupported("xml_attribute ***")] } to Bool
```

### A evidência

| n | causa de bailout |
| ---: | --- |
| 702 | conversão de `xml_attribute***` callback → `Bool` |
| 224 | idem, `xml_node***` |
| 74 | idem, `xpath_node***` |

**1.000 bailouts**, e o dano é maior que o número: cada um deles é a *condição*
de um `if` numa função `Read*`/`Write*` de `libmei/dist/`, e a função inteira
depende dela. `.diagnosis/dart-package/lib/atts_shared.dart:25`:

```dart
if (_syntaxBridgeUnsupported<bool>('…: unsupported implicit conversion from Callback { … } to Bool')) {
  SetFunc(StrToAccidLogFunc(element.attributeNullableStringConst('func').value()));
  …
}
```

O corpo do `if` está traduzido; a condição, não. **Nenhum atributo MEI é lido.**

### O que fazer

1. Lowerar `CXCursor_ConversionFunction` como um método-ponte nomeado do
   registro. O nome precisa ser estável e derivado do tipo de destino, do mesmo
   jeito que `dart_operator_bridge_name` deriva nomes de operador
   (`crates/server/src/lower/cpp.rs:7728`): `toBool`, `toInt`, `toStr`. Um
   registro com duas conversões para o mesmo destino Dart passa pela mesma
   desambiguação por tipo que a tarefa 03 estabelece.

2. **Reconhecer o idioma *safe bool* pelo que ele é.** O tipo de destino
   (`void (*)(X***)`) não interessa a ninguém: o que o C++ está dizendo é
   "este objeto é testável como booleano". Toda conversão cujo destino não
   tenha representação Dart mas que apareça em **posição de condição** deve
   virar `toBool()`. O caminho mais robusto é reconhecer a *posição*: a
   conversão implícita reportada pelo `libclang` numa condição de
   `if`/`while`/`for`/`?:`/operando de `&&`/`||`/`!`.

3. Onde a conversão aparece em posição de **valor** e o destino tem tipo Dart
   (`operator int()`, `operator std::string()`), emitir a chamada ao
   método-ponte correspondente.

4. Onde nem uma coisa nem outra, bailout **tipado** com o tipo estático
   esperado — nunca `SyntaxBridgeOpaque` cru (`AGENTS.md`, e a tarefa 06 do
   lote anterior já estabeleceu esse padrão).

## Metade B — classes que herdam de um tipo de biblioteca

### A causa raiz

`lower::cpp::base_classes_of` (`crates/server/src/lower/cpp.rs`, o filtro por
volta da linha 1487) descarta toda base cujo tipo não seja `Type::Record` ou
`Type::Enum`:

```rust
match lower_type(base_type) {
    ir::Type::Record { .. } | ir::Type::Enum { .. } => Some(ir::BaseClass { usr, name }),
    _ => None,
}
```

O comentário de 27 linhas acima dele justifica o filtro (evitar
`class HumdrumToken with string`, que não é Dart válido) e é honesto sobre a
consequência: "o uso de `HumdrumToken` como `Str` continua sendo o seu próprio
bailout honesto".

O problema é o volume desse "bailout honesto".

### A evidência

| n | conversão implícita sem lowering |
| ---: | --- |
| 1.181 | `HumdrumToken?` → `String?` |
| 865 | `HumdrumToken` → `String` |
| 249 | `GridStaff?` → `List<GridVoice?>?` |
| 170 | `GridSlice?` → `List<GridPart?>?` |
| 150 | `GridPart?` → `List<GridStaff?>?` |
| 108 | `GridMeasure?` → `List<GridSlice?>?` |
| 88 | `HumGrid` → `List<GridMeasure?>` |
| 98 | `MidiMessage` → `List<int>` |

**~2.900 bailouts.** As declarações no C++:

```cpp
class HumdrumToken : public std::string, public HumHash { … };   // humlib.h:1473
class GridStaff : public std::vector<GridVoice*>, public GridSide { … };  // humlib.h:4746
```

`HumdrumToken` **é** o texto do token — todo `*token == "*-"`, `token->find(...)`,
`token->substr(...)` do humlib passa por essa herança.

### O que fazer

A resposta do `AGENTS.md` é um **adaptador nomeado**, e a forma mais direta é
**composição em vez de herança**:

1. Um registro que herda de um tipo de biblioteca ganha um **campo** com o
   valor da base, de tipo já mapeado, com nome estável:

   ```dart
   class HumdrumToken extends HumHash {
     String syntaxBridgeStringBase = '';
     …
   }

   class GridStaff extends GridSide {
     List<GridVoice?> syntaxBridgeListBase = [];
     …
   }
   ```

2. Toda conversão implícita do objeto para o tipo da base vira acesso a esse
   campo. É exatamente a mesma conversão que hoje vira bailout: o ponto de
   inserção já existe e já sabe o par (origem, destino).

3. Toda chamada de método **da base** sobre o objeto (`token->find(x)`,
   `token->substr(a, b)`, `staff->size()`, `staff->at(i)`) vira a mesma chamada
   sobre o campo. Isso passa pelo caminho de `lower_stdlib_method_call`
   (`cpp.rs:8212`), que hoje decide pelo tipo do receptor: quando o receptor
   for um registro com base de biblioteca, ele precisa redirecionar para o
   campo antes de resolver o método.

4. **Não remova o filtro de `base_classes_of`.** Ele continua certo: a base de
   biblioteca não vira `extends`. O que muda é que ela deixa de ser
   *descartada* e passa a virar um campo — registrado no IR, não inferido no
   emissor. Acrescente ao `ir::Record` um campo novo para isso (por exemplo
   `library_base: Option<Type>`), documentado como os vizinhos.

5. Uma classe pode herdar de **duas** bases de biblioteca. Nenhuma do Verovio
   herda; se aparecer, bailout explícito no registro inteiro, não uma escolha
   arbitrária.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** para a metade A:

   ```cpp
   class Caixa {
   public:
       typedef void (*safe_bool)(Caixa***);
       operator safe_bool() const;
       int valor = 0;
   };
   int usa(const Caixa &c) { if (c) return 1; return 0; }
   ```

   Verifique que a condição do `if` não é bailout.

2. **Teste que falha primeiro** para a metade B:

   ```cpp
   #include <string>
   #include <vector>
   class Token : public std::string {
   public:
       int linha = 0;
   };
   bool ehFim(const Token &t) { return t == "*-"; }
   int tamanho(const Token &t) { return (int)t.size(); }
   ```

   Verifique que nem `ehFim` nem `tamanho` viram bailout e que o Dart emitido
   não declara `class Token with string`.

3. **Teste comportamental.** Esta é a tarefa em que a diferença entre "compila"
   e "faz a mesma coisa" é mais fácil de errar. Acrescente os dois casos acima
   a um exemplo com `oracle/cases.json`.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- "Expressão sem lowering" em `.diagnosis/verovio-6.2.0.md`: queda de pelo
  menos **3.500** em relação à rodada anterior a esta tarefa.
- `grep -rc "unsupported implicit conversion from Callback" .diagnosis/dart-package/lib/`
  → **zero**.
- `grep -rc "unsupported implicit conversion from Record { usr: \"c:@N@hum@S@HumdrumToken\"" .diagnosis/dart-package/lib/`
  → **zero**.
- Erros do `dart analyze` podem **subir**: os corpos de `Read*`/`Write*` de
  `libmei/` deixam de estar dentro de um bailout e passam a ser analisados.
  Registre o antes/depois e classifique os grupos novos.

## Quando parar e perguntar

Por decisão de **produto**. Duas previsíveis:

1. A composição da metade B muda a interface pública do Dart gerado: quem tinha
   `HumdrumToken` onde esperava `String` agora precisa de
   `token.syntaxBridgeStringBase`. Um nome melhor (`text`, `value`) é mais
   legível e menos mecânico — mas colide com nomes do domínio. Pergunte, com
   exemplos.
2. Se o corpus tiver conversões implícitas em posições que nem "condição" nem
   "valor com tipo Dart" cobrem, traga a lista antes de inventar uma terceira
   regra.

Dificuldade técnica não é motivo para parar.
