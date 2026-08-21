# Tarefa 11 — A conversão `double`→`int` está no operando, não na fronteira

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F11**. Este prompt é autocontido.

## A causa raiz

Em C++, `int x = a * 0.5 + b;` é legal: a expressão é avaliada em `double` e a
conversão estreitante acontece **na atribuição**. O mesmo vale para retorno e
para passagem de argumento — a conversão está sempre na *fronteira*, nunca no
meio da expressão.

O lowering está inserindo `Expr::Convert` **por operando**. O resultado é uma
expressão que continua sendo `double` no todo, com conversões inúteis (e caras)
espalhadas nos pedaços.

## A evidência

`.diagnosis/dart-package/lib/accid.dart:155`:

```dart
int horizontalMargin = doc!.GetOptionsConst()!.m_ledgerLineExtension.GetValue()
    * unit.toDouble() + 0.5 * rightMargin.toDouble().toInt();
//                                        ^^^^^^^^^^^^^^^^^^ no-op
// ← A value of type 'double' can't be assigned to a variable of type 'int'.
```

`rightMargin.toDouble().toInt()` converte para `double` e de volta para `int`,
sem efeito, no operando errado. O `.toInt()` que faltava era no resultado da
soma inteira.

Outros: `.diagnosis/dart-package/lib/accid.dart:153` e `:184`; a lista completa
por arquivo está no Anexo A de `docs/plans/dart-analyze-verovio-6.2.0.md`.

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, 24.791
diagnósticos) atribui a esta família **153** ocorrências:

| `code` | n | mensagem |
| --- | ---: | --- |
| `invalid_assignment` | 135 | `A value of type 'double' can't be assigned to a variable of type 'int'.` |
| `argument_type_not_assignable` | 18 | `The argument type 'double' can't be assigned to the parameter type 'int'.` (14) e o inverso (4) |

Concentração de `invalid_assignment` (564 no total, dos quais estes 135):
`editortoolkit_neume.dart` (70), `zip_file.dart` (58), `pugixml.dart` (36),
`boundingbox.dart` (35), `beam.dart` (27), `svgdevicecontext.dart` (27).

Note que o Verovio usa `int` para coordenadas de layout em unidades internas e
multiplica por fatores fracionários o tempo todo — este padrão é onipresente
no corpus, não uma curiosidade.

## Onde mexer

- `crates/server/src/lower/cpp.rs` — o caminho que constrói `ir::Expr::Convert`.
  Procure a comparação de tipo externo/filho descrita no doc comment de
  `is_transparent_wrapper` (por volta da linha 4529): é ela que decide inserir
  a conversão, e é onde a fronteira certa precisa ser identificada.
- `crates/server/src/ir/mod.rs` — `Expr::Convert`.
- `crates/server/src/emit/dart.rs` — a renderização de `Expr::Convert`
  (procure o helper que encadeia o postfix `.toInt()`/`.toDouble()`, por volta
  de 2716).

A direção: a conversão pertence à fronteira onde o C++ também a aplica —
inicialização/atribuição de variável, `return`, argumento de chamada, e
atribuição a campo — e não a cada operando de uma expressão aritmética. Dentro
da expressão, as promoções seguem as regras usuais (`int * double` → `double`
nas duas linguagens; a única diferença real é `int / int`, que em C++ é divisão
inteira e em Dart é `double` — para isso o Dart tem `~/`, e o emissor já parece
usá-lo em outros lugares).

Cancelamentos como `.toDouble().toInt()` encadeados devem ser eliminados na
construção, não deixados para o `dart format`.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro. Fixture mínimo em C++:
   ```cpp
   int f(int a, int b) { int x = a * 0.5 + b; return x; }
   ```
   Verifique que o Dart emitido tem exatamente uma conversão, no resultado da
   expressão, e que a expressão inteira não carrega `.toDouble().toInt()` no
   meio. Veja `crates/server/tests/lower_cpp.rs` para o padrão de fixture.
2. Testes de não-regressão para os casos que já funcionam: `int / int` continua
   sendo `~/`; `double` para `double` não ganha conversão nenhuma;
   `static_cast<double>(i)` explícito continua produzindo a conversão que já
   produz.
3. Implemente até passar.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `invalid_assignment` com mensagem `double` ↔ `int` → **zero** (eram 135). O
  restante dos `invalid_assignment` é de outras tarefas (05 e 06).
- `argument_type_not_assignable` com mensagem `double` ↔ `int` → **zero**
  (eram 18).
- Ocorrências de `.toDouble().toInt()` no pacote emitido → **zero**.
  Verificação direta:
  `grep -rc "toDouble().toInt()" .diagnosis/dart-package/lib/`.
- Nenhum `code` novo. O risco específico é `argument_type_not_assignable` na
  direção oposta (`int` onde se espera `double`), se a remoção de conversões
  for agressiva demais.
- **Atenção à correção, não só à contagem:** mover uma conversão muda o
  resultado numérico. `(a * 0.5 + b).toInt()` e `a * 0.5.toInt() + b` dão
  valores diferentes. O comportamento correto é o do C++, e o do C++ é
  truncamento na fronteira — que é o que `.toInt()` faz em Dart (truncamento
  em direção a zero, como o C++). Confirme isso num teste com valores
  negativos.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso possível: C++ com `int` de 32 bits e
Dart com `int` de 64 bits (ou arbitrário, em `dart2js`) divergem em overflow.
Se algum ponto do corpus depender do wrap de 32 bits, isso não é uma questão
de conversão e sim de modelo de inteiros — pergunte antes de tratar como parte
desta tarefa.

Dificuldade técnica não é motivo para parar.
