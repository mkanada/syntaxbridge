# Tarefa 14 — Resíduos de emissão (limpeza)

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
família **T14**. Este prompt é autocontido.

São **onze itens independentes**, agrupados num prompt só porque cada um é uma
correção local. Faça-os na ordem abaixo (o primeiro é o mais importante) e
**commit por item**, no mesmo estilo do lote `F15/tarefa 15.N` do backlog
anterior. Se algum item revelar-se maior do que parece, pare nele, registre o
que descobriu, e siga para o próximo — não engula os outros dez.

---

## 14.1 — `m[k]++` gera Dart que não parseia

**O único arquivo do pacote que não parseia** (`humlib.dart`, 1 de 317) é este
item. Sem ele, `dart format` aborta e o arquivo inteiro fica fora de qualquer
verificação posterior.

`dart format lib/humlib.dart` diz:

```
line 59131, column 44: Illegal assignment to non-assignable expression.
        wordlist.putIfAbsent(word, () => 0)++;
```

`Expr::MapIndexOrInsert` é emitido como `.putIfAbsent(k, () => v)`
(`crates/server/src/emit/dart.rs:3893`) — correto em posição de **leitura**,
inválido como alvo de `++`/`--`/`+=`.

O C++ é `wordlist[word]++;` (`std::map` com `operator[]` que insere). A
tradução correta:

```dart
wordlist[word] = (wordlist[word] ?? 0) + 1;
```

Reconheça `MapIndexOrInsert` em posição de alvo de incremento/decremento/
atribuição composta e emita essa forma. Ocorrências: 4
(`illegal_assignment_to_non_assignable`), mas o custo é um arquivo inteiro.

---

## 14.2 — Bailout tipado `void` em posição de valor

**226 `use_of_void_result`** — 223 em `humlib.dart`, todas na linha 20593:

```dart
return <SyntaxBridgePair<String, String>>[
  _syntaxBridgeUnsupported<void>('…: unsupported expression cursor kind 119'), …
```

O bailout está dentro de um literal de lista cujo tipo de elemento é conhecido
(`SyntaxBridgePair<String, String>`), mas foi tipado como `void`. O `AGENTS.md`
exige que "bailouts de expressão preservem o tipo estático esperado".

O cursor kind 119 é `CXCursor_InitListExpr` — um `{...}` aninhado dentro de
outro `{...}` (**356 bailouts** no total, item 14.10). Aqui basta corrigir o
**tipo** do bailout: onde ele cai num elemento de literal de lista/mapa, o tipo
esperado é o do elemento, não `void`.

---

## 14.3 — `!` redundante que a promoção não alcança

**2.472 `unnecessary_non_null_assertion`** em 111 arquivos. O mecanismo
existe e funciona: `emit::dart::receiver_bang`
(`crates/server/src/emit/dart.rs:3104`) marca o primeiro uso de um `Ref`
anulável como promovido e suprime o `!` nos seguintes.

O que ele ainda não vê são as promoções que o **próprio Dart** faz e o emissor
não modela:

1. **Dentro de uma condição composta**: `chord != null && chord!.Has…`
   (`.diagnosis/dart-package/lib/accid.dart:156`). Depois de `x != null` num
   `&&`, o Dart já promoveu `x` no operando seguinte.
2. **Dentro do corpo de um `if (x != null)`**.
3. **Depois de um `return`/`continue`/`throw` num `if (x == null)`** (early
   return).

Trate os três: uma comparação `x != null` (ou `null != x`) promove `x` no
operando seguinte de um `&&`, no ramo `then` de um `if`, e no resto do bloco
quando o ramo oposto termina o fluxo. Mantenha a regra existente para campos
(`this._m_x`) — o Dart nunca os promove.

São avisos, não erros; mas são 64% de todo o ruído de aviso do relatório.

---

## 14.4 — `import` não usado

**231 `unused_import`** em 102 arquivos. `.diagnosis/dart-package/lib/accid.dart`
importa `attdef.dart`, `glyph.dart` e `options.dart` sem usar nenhum.

O emissor já tem a coleta de USRs referenciados
(`collect_referenced_usrs_in_expr`/`_in_type`,
`crates/server/src/emit/dart.rs:940-1030`) — o import deve sair dela, não da
lista de includes do C++. Verifique por que alguns escapam: provavelmente um
tipo aparece só em posição que a coleta não visita (parâmetro de um método
cujo corpo virou bailout — a tarefa 15.2 do lote anterior tratou um caso
parecido).

---

## 14.5 — `dead_code`

**40 ocorrências**, em 12 arquivos. Duas formas conhecidas:
`if (false) { … }` (uma macro que virou constante) e statement depois de um
terminador. Leia `.diagnosis/dart-package/lib/adjustslursfunctor.dart:183,316,431`
e `beam.dart:315,324` antes de generalizar — a tarefa 15.1 do lote anterior já
tratou o `break` depois de `return` num `case`, então estes são outra coisa.

---

## 14.6 — `double` → `int` na fronteira

**~130 diagnósticos** (85 `invalid_assignment` `'double'`→`'int'`, 25
`argument_type_not_assignable` do mesmo par, 10 para `int?`, 12 no sentido
inverso). A tarefa 11 do lote anterior aplicou a conversão na fronteira de
atribuição; o que sobrou são as fronteiras que ela não cobriu: **inicialização
de campo**, **argumento de chamada** e **retorno**.

`.diagnosis/dart-package/lib/accid.dart:186` e `adjustarpegfunctor.dart:64` são
os exemplos. Em C++, `int x = a * 0.5;` é narrowing implícito legal, e a
conversão acontece na **atribuição**; a mesma regra vale nas outras três
posições.

---

## 14.7 — `unused_field`

**717 `unused_field`** em 40 arquivos. Um campo privado declarado e nunca lido
**no arquivo onde foi declarado**. Antes de mexer, **meça a causa**: pode ser
(a) o campo é lido só por uma subclasse em outro arquivo — o que seria uma
regressão de T2/tarefa 03 do lote anterior; (b) o método que o lia virou
bailout; (c) o campo é genuinamente morto no C++ também.

Se for (b), o número cai sozinho conforme as tarefas 06 a 13 avançarem, e a
resposta certa aqui é **não fazer nada** e registrar. Se for (a), é bug e tem
de ser consertado. Não silencie o aviso.

---

## 14.8 — `goto` e rótulos

**64 bailouts** `unsupported statement cursor kind 210` (`CXCursor_GotoStmt`) e
**10** de `kind 201` (`CXCursor_LabelStmt`), concentrados em `zip_file.hpp` e
`pugixml.cpp`. Dart não tem `goto`.

O subconjunto traduzível, e o único que vale a pena: `goto` para um rótulo que
está **depois** de todo o bloco corrente e é usado como "sair daqui"
(`goto cleanup;`) vira um `break` de rótulo Dart (`saida: { … break saida; … }`)
ou uma extração para função. Qualquer `goto` para trás, ou que atravesse
escopo, continua bailout — mas com mensagem **específica** dizendo qual das duas
coisas ele é. Meça a proporção antes de implementar.

---

## 14.9 — Lambdas

**145 bailouts** `unsupported expression cursor kind 144`
(`CXCursor_LambdaExpr`), mais os 28+ tipos `(lambda at …)` na tabela de tipos
sem mapeamento.

Dart tem função anônima com a mesma semântica de captura por valor
(`(a, b) => …`). O caso que **não** tem equivalente direto é a captura por
referência (`[&]`), que em Dart é sempre por referência de closure — o que é
mais permissivo, não menos, então é seguro. Comece pelo caso mais comum no
corpus (`std::sort` com comparador, `find_if` com predicado — veja
`include/vrv/comparison.h:608`) e deixe capturas explícitas exóticas
(`[x = std::move(y)]`) em bailout.

---

## 14.10 — `InitListExpr` aninhado

**356 bailouts** `unsupported expression cursor kind 119`. `ir::Expr::ListLiteral`
já existe e já documenta que "o initializer não compartilha a forma plana de um
valor por elemento" (`crates/server/src/ir/mod.rs:264-284`). O que falta é o
caso aninhado: `{{1, 2}, {3, 4}}` e `{{"a", "b"}, {"c", "d"}}`
(`humlib.cpp:454`, um `std::vector<std::pair<string, string>>`).

A regra: um `InitListExpr` cujo elemento é outro `InitListExpr` lowera cada
elemento recursivamente contra o tipo de elemento do contêiner externo — e um
elemento cujo tipo alvo é `SyntaxBridgePair`/`Tuple` vira a construção
correspondente, não uma lista.

---

## 14.11 — `const_cast`

**107 bailouts** `unsupported expression cursor kind 127`
(`CXCursor_CXXConstCastExpr`). Dart não tem `const` em tipo de referência, então
`const_cast` é um **invólucro transparente**: o valor passa e o tipo não muda.
`lower::cpp::is_transparent_wrapper` (`crates/server/src/lower/cpp.rs:4542`) já
existe e já lista `CXXStaticCastExpr`/`CStyleCastExpr`; acrescente
`CXXConstCastExpr`.

Confirme que a tarefa 05 do lote anterior (downcast preservado) não é afetada:
`const_cast` **nunca** muda o registro, só a constness, então ele não cai no
caso que aquela tarefa protege.

---

## Método

TDD para cada item, conforme `AGENTS.md`: um teste que falha primeiro (estilo
`crates/server/tests/lower_cpp.rs` / `emit_dart.rs`), depois a implementação,
depois `just test` (ou `just test-host`, registrando), `just check`,
`just lint`. Um commit por item, com a mensagem nomeando o item
(`T14/tarefa 14.1`, …).

## Critério de sucesso

Depois de `just verovio-diagnosis`, por item:

| item | métrica | de → para |
| --- | --- | --- |
| 14.1 | "Arquivos que não parseiam" | **1/317 → 0/317** |
| 14.2 | `use_of_void_result` | 226 → 0 |
| 14.3 | `unnecessary_non_null_assertion` | 2.472 → abaixo de 400 |
| 14.4 | `unused_import` | 231 → 0 |
| 14.5 | `dead_code` | 40 → 0 |
| 14.6 | `invalid_assignment` `double`→`int` | 85 → 0 |
| 14.7 | `unused_field` | medido e explicado (pode não cair) |
| 14.8 | bailouts `statement cursor kind 210/201` | 74 → medido, com a proporção registrada |
| 14.9 | bailouts `expression cursor kind 144` | 145 → abaixo de 40 |
| 14.10 | bailouts `expression cursor kind 119` | 356 → 0 |
| 14.11 | bailouts `expression cursor kind 127` | 107 → 0 |

Em todos: nenhum `code` novo, e nenhuma das três contagens de bailout sobe sem
justificativa registrada.

## Quando parar e perguntar

Só por decisão de **produto**. Os dois candidatos: o item 14.8 (`goto`), se a
proporção de `goto` para trás for alta — a alternativa seria reestruturar o
fluxo, o que é uma tarefa própria; e o item 14.7 (`unused_field`), se a medição
mostrar que campos estão sumindo por outra causa.

Dificuldade técnica não é motivo para parar.
