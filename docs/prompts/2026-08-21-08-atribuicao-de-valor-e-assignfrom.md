# Tarefa 08 — `assignFrom` é chamado 483 vezes e nunca é declarado

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F5**. Este prompt é autocontido.

## A causa raiz

`lower::cpp::dart_operator_bridge_name` (`crates/server/src/lower/cpp.rs`, por
volta da linha 6285) dá a cada operador C++ que não tem forma declarável em
Dart um nome-ponte estável:

```rust
match symbol {
    "<<" => "streamInsert",
    ">>" => "streamExtract",
    "=" => "assignFrom",
    …
}
```

O doc comment diz que o nome é "shared with the emitter so every declaration
and call site keeps the same target". Para `operator=` isso não se sustenta:

- Na maioria absoluta dos casos reais, o `operator=` que o libclang resolve é o
  **implícito** — a cópia membro a membro que o compilador gera. Ele não tem
  cursor de definição, então não há nada para lowerar, e **a declaração de
  `assignFrom` nunca é emitida**.
- Quando é um `operator=` explícito, ele costuma estar definido out-of-line num
  `.cpp` — e aí a causa raiz da tarefa 01 o descarta junto com os outros
  membros out-of-line.

O call site, por outro lado, é sempre emitido. E não como método: como
**chamada livre de dois argumentos**, com o lado esquerdo virando o primeiro
argumento.

## A evidência

`.diagnosis/dart-package/lib/alignfunctor.dart:67`, dentro do construtor de
`AlignHorizontallyFunctor`:

```dart
assignFrom(_m_time, Fraction(0));
```

O C++ correspondente é `m_time = Fraction(0);`.

`.diagnosis/dart-package/lib/iomei.dart:354`:

```dart
assignFrom(_m_currentNode, _m_currentNode.append_child(name));
```

`.diagnosis/dart-package/lib/humlib.dart:3059`:

```dart
assignFrom(duration, -1);
```

Como todas estão dentro de métodos, o Dart lê `assignFrom(...)` como
`this.assignFrom(...)` — daí o erro sair como `undefined_method`, não como
`undefined_function`.

Números de `.diagnosis/verovio-6.2.0.analyze.json` (24.791 diagnósticos):

- `undefined_method` com nome `assignFrom`: **483** ocorrências.
- Ocorrências textuais de `assignFrom(` no pacote emitido: **883**, em 32
  arquivos (as demais estão em posições que já falharam por outro motivo).
- Declarações de `assignFrom` no pacote: **zero**.

Uma parte de `argument_type_not_assignable` também vem daqui, quando o
analisador tenta casar os dois argumentos com alguma `assignFrom` de outro
escopo (`humlib.dart:3059` →
`The argument type 'HumNum' can't be assigned to the parameter type 'MSearchQueryToken'.`).

## Onde mexer

- `crates/server/src/lower/cpp.rs`:
  - `dart_operator_bridge_name` (~6285) — o mapeamento de `"="`.
  - O caminho que lowera uma chamada a operador de membro para
    `ir::Expr::Call` com `callee_name` vindo de `dart_member_name`
    (por volta de 6255) — é ele que produz a forma livre de dois argumentos.
  - O ponto onde uma atribuição C++ (`CXCursor_BinaryOperator` com `=`, ou o
    `CXXOperatorCallExpr` correspondente) é lowered.

A direção da correção é separar dois casos que hoje são um só:

**Caso 1 — `operator=` implícito.** É a cópia gerada pelo compilador. Aqui é
preciso decidir o que "atribuição por valor" vira em Dart, onde toda variável
de tipo classe é uma referência:

- Quando o lado direito é um **temporário recém-construído** (`Fraction(0)`,
  `x.append_child(n)`, uma chamada que devolve valor novo) — a esmagadora
  maioria dos casos reais deste corpus — atribuição simples (`_m_time = Fraction(0);`)
  é uma tradução **correta**: não há aliasing possível com um objeto que
  ninguém mais referencia.
- Quando o lado direito é um **objeto vivo** (outra variável, um campo), C++
  copia e Dart aliasaria. Aí é preciso uma cópia explícita — um método
  `copyFrom` gerado no registro, campo a campo — ou um bailout honesto.

**Caso 2 — `operator=` explícito escrito pelo usuário.** Aqui `assignFrom` é o
nome-ponte certo, mas precisa ser emitido como **método do registro**
(`alvo.assignFrom(origem)`), e a declaração precisa existir. Depois da tarefa
01, os `operator=` out-of-line voltam a chegar ao registro; verifique se o
emissor de métodos já lida com um membro cujo nome veio de
`dart_operator_bridge_name`.

Os outros nomes-ponte (`streamInsert`, `increment`, `addAssign`, …) sofrem do
mesmo padrão em menor escala; se a correção puder cobri-los sem inchar,
melhor — mas o alvo desta tarefa é `assignFrom`.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro, caso 1: uma `struct` C++ sem `operator=`
   declarado, atribuída a partir de um temporário
   (`Ponto p; p = Ponto(1, 2);`). Verifique que o Dart emitido é uma atribuição
   simples e não uma chamada a `assignFrom`. Veja
   `crates/server/tests/lower_cpp.rs` para o padrão de fixture.
2. Teste que falha, caso 2: uma `struct` com `operator=` explícito. Verifique
   que a declaração é emitida **e** que o call site a alcança.
3. Teste do caso perigoso: atribuição a partir de um objeto vivo
   (`Ponto a, b; a = b;`). O que quer que seja emitido, não pode ser uma
   chamada a algo inexistente.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- Ocorrências de `assignFrom(` no pacote emitido em posição de chamada livre:
  **zero**. Verificação direta:
  `grep -rn "assignFrom(" .diagnosis/dart-package/lib/` só pode devolver
  chamadas na forma `alvo.assignFrom(origem)` com a declaração correspondente
  existindo.
- `undefined_method` cai em ~483 em relação à linha de base daquele momento.
- Nenhum `code` novo.
- Se você escolher emitir `copyFrom` para o caso de objeto vivo, a contagem de
  bailouts em `.diagnosis/verovio-6.2.0.md` não deve subir; se escolher
  bailout, ela sobe e isso precisa estar registrado no resumo.

## Quando parar e perguntar

**Este prompt tem uma decisão de produto real e você deve levá-la ao usuário.**

O caso "lado direito é um objeto vivo" (`a = b;` com `b` sendo outra variável)
não tem tradução única:

- **Opção A** — atribuição simples sempre. Dart aliasaria onde C++ copiava:
  mutações posteriores em `b` passariam a ser visíveis por `a`. Silenciosamente
  errado em alguns casos, correto na maioria dos casos deste corpus, e é o Dart
  mais idiomático.
- **Opção B** — gerar `copyFrom` em todo registro com semântica de valor e
  emitir `a.copyFrom(b)`. Preserva a semântica C++, ao custo de código gerado
  mais pesado e de precisar decidir o que "cópia" significa para campos que são
  ponteiros (cópia rasa, como o C++ implícito faz).
- **Opção C** — atribuição simples quando o lado direito é comprovadamente um
  temporário, `copyFrom` quando não é. Mais correto, mais complexo.

Recomendação: **C**, com A como comportamento quando a análise não conseguir
provar que é temporário só se o usuário aceitar o risco de aliasing. Mas
pergunte: as três mudam comportamento observável do produto.

Dificuldade técnica não é motivo para parar.
