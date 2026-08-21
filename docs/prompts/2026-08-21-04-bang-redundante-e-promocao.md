# Tarefa 04 — Não repetir `!` onde o Dart já promoveu o valor a não-nulo

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido). Veja também
`docs/plans/estilo-de-codigo-gerado.md`: o código Dart emitido é produto, não
resíduo, e ruído nele conta.

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F3**. Este prompt é autocontido.

## A causa raiz

Um ponteiro C++ (`T*`) é lowered para `Type::Nullable(T)` e desreferenciado em
Dart com `!`. A escolha de **asserção** em vez de checagem está correta e bem
justificada — o comentário de `emit::dart::receiver_bang` explica: C++ nunca
exigiu (nem ofereceu) checagem de nulo para `p->x`, então propagar a exigência
do Dart para dentro seria inventar uma decisão que o C++ de origem não tomou.
Isso **não** é o problema.

O problema é que o `!` é decidido **puramente pelo tipo estático do IR**, sem
nenhuma noção de fluxo (`crates/server/src/emit/dart.rs`, por volta da linha
2708):

```rust
fn receiver_bang(receiver: &Expr) -> &'static str {
    if matches!(expr_ty(receiver), Some(Type::Nullable(_))) { "!" } else { "" }
}
```

O Dart tem **promoção de tipo sensível a fluxo**: depois de `x!` ou de
`x != null` numa condição que domina o uso, `x` é tratado como não-nulo para o
resto daquele fluxo — desde que `x` seja uma variável local ou parâmetro (o
Dart nunca promove campos, porque outro código pode reatribuí-los). Como o
emissor não sabe disso, ele repete o `!` em cada dereferência, e o analisador
reclama de cada repetição.

## A evidência

`.diagnosis/dart-package/lib/accid.dart`:

```dart
149:  void AdjustToLedgerLines(Doc? doc, LayerElement? element, int staffSize) {
150:    Staff? staff = element!.GetAncestorStaff(StaffSearch.RESOLVE_CROSS_STAFF);
151:    Chord? chord = GetFirstAncestor(ClassId.CHORD);
152:    int unit = doc!.GetDrawingUnit(staffSize);
153:    int rightMargin = doc!.GetRightMargin(ClassId.ACCID) * …;      // ← 1 aviso
154:    if (element!.IsClassId(…) && chord != null && chord!.HasAdjacentNotesInStaff(staff)) {  // ← 3 avisos
155:      int horizontalMargin = doc!.GetOptionsConst()!…              // ← 1 aviso
156:      int staffTop = staff!.GetDrawingY();
157:      int staffBottom = staffTop - doc!.GetDrawingStaffSize(staffSize);  // ← 1 aviso
```

Depois do `element!` da linha 150, `element` está promovido; o `!` da 154 é
redundante. Depois do `doc!` da 152, todos os `doc!` seguintes são redundantes.
E `chord!` na 154 vem logo depois de `chord != null` no mesmo `&&`.

Outro exemplo, `.diagnosis/dart-package/lib/adjustbeamsfunctor.dart:196`:
`if (rest!.GetDots() > 0) {`.

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`):

- `unnecessary_non_null_assertion` — **6.107 ocorrências**, em 77 arquivos, com
  uma única mensagem: `The '!' will have no effect because the receiver can't be null.`
- Concentração: `iomei.dart` (1196), `iohumdrum.dart` (1082), `zip_file.dart`
  (577), `editortoolkit_neume.dart` (541).

São 24,6% de todos os 24.791 diagnósticos do relatório, e o segundo `code` mais
frequente. São **avisos**, não erros: nada quebra hoje. O que se ganha é código
gerado legível.

## Onde mexer

- `crates/server/src/emit/dart.rs` — `receiver_bang` é o ponto de decisão, mas
  ele não tem contexto suficiente sozinho. Vai ser preciso um estado de
  emissão por corpo de função (o emissor já percorre `Vec<Stmt>` em
  `emit_stmt`), carregando o conjunto de nomes atualmente promovidos.
- Cuidado com `tuple_assign_needs_temp_block` (mesmo arquivo), que consulta
  `receiver_bang` para decidir a forma de `Stmt::TupleAssign`. Se `receiver_bang`
  passar a depender de contexto, esse chamador precisa do mesmo contexto — ou de
  uma variante que continue perguntando só pelo tipo.

Um subconjunto conservador das regras do Dart já resolve a maior parte:

- Promove: `x!` sobre um `Expr::Ref` de local/parâmetro; `x != null` como
  condição de um `if` (dentro do `then`), como operando esquerdo de `&&`
  (para o resto do `&&` e do `then`), ou como guarda de retorno antecipado
  (`if (x == null) return;` promove `x` no resto do bloco).
- Invalida: qualquer atribuição a `x`; a saída do bloco onde a promoção nasceu;
  passar `x` como argumento não muda nada (o Dart tampouco invalida).
- **Nunca** promove: acesso a campo (`this._m_x`, `obj.campo`). O Dart não
  promove campos, então esses mantêm o `!` sempre.

Não precisa reproduzir a análise de fluxo do Dart inteira. Ser conservador
(emitir `!` em dúvida) é sempre correto: no pior caso sobra um aviso, nunca um
erro. Ser agressivo demais **é** perigoso: remover um `!` necessário vira erro
de compilação.

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro. Fixture mínimo em C++: uma função que recebe dois
   ponteiros e os desreferencia mais de uma vez em sequência. Verifique que o
   Dart emitido tem `!` na primeira dereferência de cada e não nas seguintes.
2. Um teste de segurança na direção oposta: depois de uma reatribuição
   (`p = outraCoisa();`), o `!` **volta**. E um campo (`this->m_x->f()` duas
   vezes) mantém `!` nas duas.
3. Implemente até passar.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar
no Flatpak):

- `unnecessary_non_null_assertion` — de **6107** para perto de zero. Um resíduo
  pequeno é aceitável (formas de fluxo que o subconjunto conservador não cobre);
  registre quanto sobrou e por quê.
- **Zero** ocorrências novas de `unchecked_use_of_nullable_value`. Este é o
  erro que aparece se você remover um `!` que era necessário — é o critério de
  segurança desta tarefa, e ele vale mais do que a queda do alvo. Se subir,
  a correção está agressiva demais.
- `argument_type_not_assignable` e `invalid_assignment` não podem subir: um
  `T!` removido indevidamente muda o tipo estático da expressão de `T` para `T?`.

## Quando parar e perguntar

Só por decisão de **produto**. Um caso plausível: se o usuário preferir uma
abordagem estruturalmente diferente — por exemplo, ligar o parâmetro nulável a
uma local não-nulável no topo do método (`final e = element!;` e usar `e`
depois), o que produz Dart mais idiomático mas renomeia variáveis em relação ao
C++ de origem. Isso muda a legibilidade do código gerado de forma observável;
se você achar que vale mais que a supressão do `!`, pergunte em vez de decidir.

Dificuldade técnica não é motivo para parar.
