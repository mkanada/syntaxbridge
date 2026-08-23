# Tarefa 10 — Out-param: o local ainda sai `late` sem valor

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório; `dynamic` proibido; **nunca deixar `late` sem
escrita** — é erro de compilação, não bailout honesto).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/estado-da-transpilacao-verovio-6.2.md`,
família **T10**. Este prompt é autocontido.

Esta é uma tarefa **pequena e bem delimitada**: o mecanismo certo já existe e
está correto; ele só não alcança um caso.

## A causa raiz

O produto já tem a ponte de out-param: um `void f(int &a, int &b)` do C++ vira
um `(int, int) f(int a, int b)` do Dart, e o call site vira
`(x, y) = f(x, y);` (`ir::Stmt::TupleAssign`).

O efeito colateral é que `int x, y;` do C++ — declaração sem inicializador —
vira `late int x;` no Dart, e a chamada **lê** `x` como argumento antes de
qualquer escrita. Isso é `definitely_unassigned_late_local_variable`.

A correção existe: `lower::cpp::neutralize_out_param_call_input_locals`
(`crates/server/src/lower/cpp.rs:588`, com um doc comment de 30 linhas
explicando exatamente este problema). Ela varre, a partir de cada `VarDecl` sem
inicializador, os statements **seguintes**, e quando acha um `TupleAssign` que
usa aquele local como argumento *e* como alvo, dá a ele o valor neutro do tipo.

O limite está na varredura: ela só olha statements **no mesmo nível de
aninhamento**. `recurse_neutralize_out_param_call_input_locals` (linha 645)
desce para dentro de `if`/`while`/`for`/`try`, mas cada descida recomeça a
varredura naquele bloco — um `VarDecl` no bloco de fora nunca é visto pela
varredura de dentro.

## A evidência

**298 `definitely_unassigned_late_local_variable`** em 23 arquivos (eram 179
antes da tarefa 10 do lote anterior: a ponte passou a funcionar em mais lugares,
e o sintoma seguiu junto). Mais **377 `unused_local_variable`**, que é o mesmo
fenômeno visto do outro lado.

`.diagnosis/dart-package/lib/adjustclefchangesfunctor.dart:76-84`:

```dart
late int nextLeft;
late int nextRight;
if (graceAligner != null) {
  nextLeft = graceAligner!.GetGraceGroupLeft(staff!.GetN());
} else {
  (nextLeft, nextRight) = nextAlignment.GetLeftRightListIntIntIntListClassIdConst(ns, nextLeft, nextRight);
}
if (nextLeft == -(-2147483647)) {
```

O C++ (`src/adjustclefchangesfunctor.cpp:101-109`):

```cpp
int nextLeft, nextRight;
if (graceAligner) {
    nextLeft = graceAligner->GetGraceGroupLeft(staff->GetN());
}
else {
    nextAlignment->GetLeftRight(ns, nextLeft, nextRight);
}
if (nextLeft == -VRV_UNSET) nextLeft = nextAlignment->GetXRel();
```

A declaração está no nível de fora; o `TupleAssign` está dentro do `else`.

O mesmo padrão em `adjustslursfunctor.dart:277`, `adjustclefchangesfunctor.dart:81`,
e nos outros 21 arquivos.

## O que fazer

1. **Estender a varredura para atravessar aninhamento.** A partir de cada
   `VarDecl` sem inicializador, a busca precisa considerar não só os
   statements irmãos seguintes, mas também o **interior** deles (o corpo de um
   `if`, de um `while`, de um `for`, os dois ramos de um `if/else`, o corpo de
   um `try`).

   A condição de parada tem de continuar valendo: se, antes de qualquer uso
   como argumento-de-ponte, o local receber um `Assign`/`ExprAssign` **em todos
   os caminhos**, o `late` já estava correto e não se deve mexer. Se o
   `Assign` acontecer só em um ramo (como no exemplo acima, em que o ramo
   `then` atribui e o `else` usa como argumento), o local **precisa** do valor
   neutro.

   A regra segura e simples: se existe **qualquer** uso do local como
   argumento-de-entrada de um `TupleAssign` que também o tem como alvo, em
   qualquer profundidade dentro do escopo da declaração, e esse uso não é
   precedido por uma atribuição que o domine, dê o valor neutro. Um valor
   neutro a mais nunca quebra nada — em C++ aquele local tinha valor
   indeterminado de qualquer forma; um `late` sem escrita quebra a compilação.

2. **Considere a alternativa mais simples e mais segura.** Se distinguir
   "dominado por uma atribuição" custar caro, a regra "todo `VarDecl` escalar
   sem inicializador ganha o valor neutro do tipo" está sempre **correta** — em
   C++ o valor era indeterminado, e o Dart não tem indeterminado. O custo é
   perder o diagnóstico do Dart para o caso "li antes de escrever", que era um
   bug real no C++ original. Meça as duas: se a diferença for de poucas dezenas
   de ocorrências, prefira a simples e registre a escolha.

3. **Não mexa em `late` de campo.** Um campo `late` tem outra história
   (`no default value available for this field's type yet`, 258 bailouts) e não
   é desta tarefa.

4. **Os `unused_local_variable`.** Quando a ponte devolve uma tupla e o call
   site só usa um dos valores, o outro fica declarado e nunca lido. Verifique
   quantos dos 377 são disso; se forem a maioria, o descarte de tupla do Dart
   (`_`) é a resposta — e `is_tuple_assign_discard` já existe em
   `crates/server/src/emit/dart.rs`. Se forem outra coisa, deixe para a tarefa
   14 e registre.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** (estilo `crates/server/tests/lower_cpp.rs` +
   `emit_dart.rs`), o formato exato do `adjustclefchangesfunctor.cpp`:

   ```cpp
   struct Alvo {
       void limites(int n, int &esq, int &dir) const;
   };
   int usa(const Alvo &a, bool cedo) {
       int esq, dir;
       if (cedo) {
           esq = 0;
       } else {
           a.limites(1, esq, dir);
       }
       return esq + dir;
   }
   ```

   Verifique que o Dart emitido não contém `late int esq;` sem valor e que
   `dart analyze` sobre o pacote não reporta
   `definitely_unassigned_late_local_variable`.

2. **Teste do caso que não deve mudar**: um local sem inicializador que recebe
   um `Assign` simples logo em seguida deve continuar `late` (ou não — mas o
   teste tem de fixar qual, para a decisão ficar registrada).

3. **Teste de aninhamento profundo**: a mesma coisa dentro de um `for` dentro
   de um `if`.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`. `examples/E10-ponteiros-union-out-params/` é a
   rede de segurança e tem oráculo.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `definitely_unassigned_late_local_variable`: **298 → 0**.
- `unused_local_variable`: **377 → cai**, ou fica igual com a explicação
  registrada.
- `grep -rc "late int \|late double \|late bool " .diagnosis/dart-package/lib/`
  → cai bastante; os que sobrarem devem ser justificáveis um a um.
- Nenhum `code` novo; nenhuma das três contagens de bailout sobe.

## Quando parar e perguntar

Só por decisão de **produto**: a escolha entre a regra fina (item 1) e a regra
simples (item 2) muda quantos bugs do C++ original o Dart gerado ainda denuncia.
Se a diferença for grande, traga os números e pergunte. Se for pequena, decida,
registre e siga.

Dificuldade técnica não é motivo para parar.
