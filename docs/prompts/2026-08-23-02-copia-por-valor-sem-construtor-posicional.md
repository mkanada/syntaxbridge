# Tarefa 02 — Cópia por valor chama um construtor posicional que não existe

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
família **T2**. Este prompt é autocontido.

**Faça a tarefa 01 antes desta**, se ainda não tiver sido feita: ela muda como
construtores são emitidos, e esta tarefa mexe no mesmo lugar.

## A causa raiz

Em C++, um parâmetro por valor é uma **cópia**. Em Dart, todo objeto é
referência. O produto resolve isso desde o exemplo E03 copiando na entrada da
função: `lower::cpp::collect_params_with_clone_prelude`
(`crates/server/src/lower/cpp.rs:2819`) insere, como primeiro statement do
corpo, uma reconstrução campo a campo:

```rust
prelude.push(ir::Stmt::Assign {
    name: param_name.clone(),
    value: ir::Expr::RecordConstruct { type_usr, type_name, fields: field_values, … },
    …
});
```

`emit::dart` (`crates/server/src/emit/dart.rs:3832`) imprime `RecordConstruct`
literalmente como `Tipo(arg1, arg2, …)`. Isso pressupõe o **construtor
posicional sintético** — `Ponto(this.x, this.y);` — que `emit_record` só emite
quando o registro **não tem construtor próprio**. O comentário em
`crates/server/src/emit/dart.rs:1096-1112` é explícito sobre isso: "as duas
formas não se misturam no mesmo registro".

Para o Verovio, a esmagadora maioria dos registros **tem** construtor próprio.
A cópia gerada chama, então, um construtor que aceita outra coisa — ou nada.

Há um segundo dano, independente: a cópia lê os campos um a um, e campos
`private` em Dart são privados **de biblioteca**. `HumNum._top` é lido de
`iohumdrum.dart`; `HumNum` mora em `humlib.dart`.

## A evidência

`dart analyze` sobre o pacote (`.diagnosis/verovio-6.2.0.analyze.json`, commit
`32dd1df`) atribui a esta família ~2.200 diagnósticos:

| `code` | n | detalhe |
| --- | ---: | --- |
| `extra_positional_arguments` | 2.080 | 1.329 são "0 esperados, 1 encontrado" |
| `undefined_getter` | ~120 | `_top`/`_bot` de `HumNum` (64), `_m_numerator`/`_m_denominator` de `Fraction` (54) |
| `not_enough_positional_arguments` | ~14 | o mesmo erro com o sinal trocado |

Concentração de `extra_positional_arguments` por arquivo: `iomei.dart` 399,
`atts_shared.dart` 380, `humlib.dart` 268, `editortoolkit_neume.dart` 204,
`atts_visual.dart` 98.

Caso 1 — `.diagnosis/dart-package/lib/atts_analytical.dart:25-26`:

```dart
bool ReadHarmAnl(xml_node element, [bool removeAttr = true]) {
  element = xml_node(element.root);
```

`xml_node` declara `xml_node()` (0 parâmetros) e `xml_node.ctor2(xml_node_struct? p)`.
A chamada gerada não bate com nenhum dos dois. O C++ correspondente
(`libmei/dist/atts_analytical.cpp:25`) é apenas
`bool AttHarmAnl::ReadHarmAnl(pugi::xml_node element, bool removeAttr)` — a
cópia é 100% sintética.

Caso 2 — `.diagnosis/dart-package/lib/iohumdrum.dart:149-151`:

```dart
void setMeterBottom(HumNum meterbot) {
  meterbot = HumNum(meterbot._top, meterbot._bot);
  _m_meter_bottom.assignFromHumNum(meterbot);
}
```

Dois erros na mesma linha: `HumNum` tem quatro construtores próprios (nenhum
deles `(int, int)` posicional na posição primária), e `_top`/`_bot` são
inacessíveis de `iohumdrum.dart`.

## Onde mexer

A direção é: **cópia por valor deixa de ser expressa como uma construção e
passa a ser um método do próprio registro.**

1. **`crates/server/src/emit/dart.rs`, `emit_record`** — emitir, para todo
   registro copiável, um método de cópia com nome estável. Sugestão:

   ```dart
   Ponto syntaxBridgeCopy() {
     final copia = Ponto();        // ou o construtor primário real
     copia.x = x;
     copia.y = y;
     return copia;
   }
   ```

   Como o método mora **dentro** da classe, ele enxerga os campos privados —
   o que resolve os ~120 `undefined_getter` por construção, não por regra nova
   de nomeação.

   O ponto delicado é a primeira linha: como obter uma instância vazia quando o
   registro só tem construtores que exigem argumentos. Duas saídas, nesta
   ordem de preferência:
   - emitir junto um construtor nomeado dedicado — `Ponto.syntaxBridgeEmpty()`
     com corpo vazio, que confia nos defaults de campo que `emit_field_declaration`
     já garante para todo registro com construtor próprio; ou
   - um construtor nomeado de cópia — `Ponto.syntaxBridgeCopyOf(Ponto other)` —
     que atribui campo a campo, dispensando o método e a instância vazia.

   Escolha **uma** forma e use-a em todos os lugares; duas formas para a mesma
   coisa é o que criou este bug.

2. **`crates/server/src/ir/mod.rs`** — `Expr::RecordConstruct` hoje carrega
   `fields: Vec<(String, Expr)>` e é usado para dois papéis diferentes
   (agregar e clonar). Separe: introduza `Expr::RecordCopy { target: Box<Expr>,
   type_usr, type_name, origin }` para o papel de cópia. Deixar os dois papéis
   na mesma variante é o que fez a mudança de `emit_record` (E04) quebrar a
   cópia (E03) sem que nada avisasse.

3. **`crates/server/src/lower/cpp.rs:2819`
   (`collect_params_with_clone_prelude`)** — passar a emitir
   `Stmt::Assign { name: p, value: Expr::RecordCopy { target: Ref(p), … } }`.
   Toda a construção do `field_values` (que hoje precisa de
   `record_fields_of(decl)`) desaparece: o emissor já conhece os campos do
   registro na hora de emitir o método de cópia.

4. **Procure outros usuários de `RecordConstruct` no papel de cópia.** Pelo
   menos `mock_value_for_type` (`emit/dart.rs`, por volta da linha 1397) e
   qualquer caminho de cópia por valor de **retorno** ou de **atribuição**
   (`operator=` implícito, tratado na tarefa 08 do lote anterior). Todos
   precisam da mesma forma.

5. **Registros que não são copiáveis.** Um registro emitido como `mixin` não
   pode ter construtor; um registro cujo campo tem tipo sem valor default
   (`no default value available for this field's type yet`, 258 bailouts hoje)
   não pode ser copiado campo a campo. Nesses casos, um bailout de statement
   honesto na posição do prelúdio — **nunca** uma cópia parcial silenciosa.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** (estilo `crates/server/tests/emit_dart.rs`), o
   caso exato que o Verovio produz — registro **com** construtor próprio,
   passado por valor:

   ```cpp
   class Fracao {
   public:
       Fracao() : _n(0), _d(1) {}
       Fracao(int n, int d) : _n(n), _d(d) {}
       int numerador() const { return _n; }
   private:
       int _n;
       int _d;
   };
   void usa(Fracao f) { f = Fracao(1, 2); }
   ```

   Verifique que o Dart emitido para `usa` **não** contém `Fracao(f._n, f._d)`
   e que o pacote inteiro passa `dart analyze` sem erro.

2. **Teste de privacidade**: o mesmo registro consumido por uma função definida
   em **outra** unidade de compilação (veja `examples/E11-multi-tu/` para o
   padrão), garantindo que o campo privado nunca é lido de fora do arquivo do
   registro.

3. **Não quebre E03.** `examples/E03-struct-pod/` é justamente a armadilha da
   cópia por valor e tem oráculo comportamental (`oracle/cases.json`). Ele
   precisa continuar passando — se o golden mudar, revise o diff com cuidado
   antes de `just examples-bless`; a mudança esperada é só a forma da cópia.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `extra_positional_arguments`: **2.080 → abaixo de 200.** O resto são chamadas
  variádicas e construções genuinamente com aridade errada, que não são desta
  família.
- `undefined_getter`: **163 → abaixo de 45** (os `_top`/`_bot`/`_m_numerator`/
  `_m_denominator` somem).
- `not_enough_positional_arguments`: cai ~14.
- Verificação direta:
  `grep -rn "= xml_node(.*\.root)" .diagnosis/dart-package/lib/` → zero
  ocorrências.
- Nenhum `code` novo; nenhuma das três contagens de bailout sobe mais que ~2%.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível: o método de cópia é raso
(copia referências de campos que são objetos), enquanto o C++ faria cópia
profunda para campos que são valores (`Fracao` dentro de `Compasso`). Copiar em
profundidade muda comportamento observável (aliasing) e custo. A regra proposta
aqui é: **copiar em profundidade os campos cujo tipo é um registro copiável, e
por referência os demais** — o que espelha o C++. Se o corpus real tiver
hierarquias em que isso ficar caro ou ambíguo, pergunte com números.

Dificuldade técnica não é motivo para parar.
