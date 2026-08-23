# Tarefa 03 — `operator[]` no call site, e overloads de operador que colidem

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
família **T3**. Este prompt é autocontido.

Esta é a tarefa com a melhor razão entre erros eliminados e linhas mexidas do
lote inteiro. Ela tem **duas metades independentes**; faça as duas.

## Metade A — a declaração usa `operator []`, o call site usa `unsupportedOperator`

### A causa raiz

O lado da **declaração** já está certo. `emit::dart::direct_dart_operator_symbol`
(`crates/server/src/emit/dart.rs:1697`) tem `[]` e `[]=` na sua tabela
`DIRECT_DART_OPERATOR_ARITIES` (linha 1684), então
`HumdrumLine& HumdrumFileBase::operator[](int index)` sai como
`HumdrumLine? operator [](int index)` — Dart válido.

O lado do **call site** não. `lower::cpp::lower_record_operator_call`
(`crates/server/src/lower/cpp.rs:10375`) só reconhece dez operadores, todos
binários:

```rust
let op = match callee_name {
    "operator+" => ir::BinaryOp::Add,
    …
    "operator>=" => ir::BinaryOp::Ge,
    _ => return None,   // ← operator[] cai aqui
};
```

Devolvendo `None`, o fluxo desce para o fallback de nome-ponte
(`cpp.rs:10627`), que chama `dart_operator_bridge_name("operator[]", 1)` — e
essa função (`cpp.rs:7728`) não tem braço para `[]`, então devolve
`"unsupportedOperator"`. O resultado é `infile.unsupportedOperator(i)` chamando
uma declaração que se chama `operator []`.

O mesmo vale para o `operator-` **unário** (`HumNum operator-(void) const`,
`humlib.h:330`): a tabela da emissão aceita aridade 0, o call site não.

### A evidência

**1.101 `undefined_method`** sobre `unsupportedOperator`, por tipo de receptor:

| n | receptor |
| ---: | --- |
| 730 | `HumdrumFile` |
| 113 | `HumdrumFileSet` |
| 90 | `MuseData` |
| 87 | `HumdrumFileBase` |
| 30 | `HumdrumFileContent` |

1.013 delas em `humlib.dart`, 88 em `iohumdrum.dart`. É o idioma `infile[i]`,
que é como todo o parsing de Humdrum navega o arquivo.

### O que fazer

Em `lower_record_operator_call`, reconhecer as formas que o Dart representa
diretamente e que hoje escapam:

| C++ | aridade | Dart |
| --- | ---: | --- |
| `a[i]` (leitura) | 1 | `ir::Expr::Index` → `a[i]` |
| `a[i] = v` | 2 (`operator[]=`) | `ir::Stmt` de atribuição indexada → `a[i] = v` |
| `-a` (unário) | 0 | `ir::Expr::Unary` com negação |
| `!a` | 0 | já existe como `logicalNot`; verifique se o call site bate |

`ir::Expr::Index` já existe (`crates/server/src/ir/mod.rs`) e já é emitido —
é o que `std::vector` usa. Reaproveite-o; não crie variante nova.

**Cuidado com o caso que hoje funciona por acidente**: se o receptor for um
`std::vector`/`std::map`, o caminho de `lower_stdlib_method_call` já tratou o
`operator[]` antes de chegar aqui. Não duplique nem intercepte antes dele —
posicione o novo braço no mesmo ponto onde `lower_record_operator_call` já é
chamado hoje.

## Metade B — overloads de operador colidem no mesmo nome Dart

### A causa raiz

Dart **não permite sobrecarga de operador por tipo de parâmetro**: uma classe
tem no máximo um `operator +`. C++ permite, e o humlib usa isso à larga
(`humlib.h:331-338`):

```cpp
HumNum operator+ (const HumNum& value) const;
HumNum operator+ (int value) const;
HumNum operator- (const HumNum& value) const;
HumNum operator- (int value) const;
…
```

O primeiro de cada par vira `operator +` de verdade; o segundo cai no
nome-ponte `unsupportedOperator`, e como vários caem no mesmo nome, colidem.
`.diagnosis/dart-package/lib/humlib.dart:815, 831, 852, 868` são quatro métodos
chamados `HumNum unsupportedOperator(int value)` na mesma classe.

A passada que deveria desambiguar isso —
`function_catalog::apply_overload_renames` (`crates/server/src/function_catalog.rs`,
por volta da linha 705) — **exclui explicitamente** esses operadores, e o
comentário no código (linhas 718-736) documenta o porquê e admite a lacuna:

> `NATIVE_BINARY_OPERATORS` … são excluídos: seus call sites nunca passam pelo
> mecanismo de `Call` com `callee_usr` … Um grupo colidente destes fica sem
> renomear, igual a antes desta correção — uma lacuna real, mas preexistente,
> que esta tarefa não alcança (precisaria que o próprio
> `lower_record_operator_call` soubesse da colisão, não só desta passada
> post-hoc).

Esta tarefa é exatamente essa lacuna.

### A evidência

**44 `duplicate_definition`** — `The name 'unsupportedOperator' is already
defined` — em 8 arquivos: `humlib.dart` (a maioria), `devicecontextbase.dart`,
`jsonxx.dart`, `pugixml.dart`, `attalternates.dart`.

### O que fazer

A regra: **para cada símbolo de operador, no máximo um overload por registro
fica com a forma nativa do Dart; os demais viram métodos nomeados,
desambiguados por tipo de parâmetro, e o call site escolhe entre os dois pelo
tipo do argumento.**

1. Decidir qual overload é o "nativo". A escolha estável e previsível: aquele
   cujo parâmetro tem o **mesmo tipo do receptor** (`HumNum + HumNum`). Se
   nenhum tiver, o primeiro em ordem de declaração. A decisão precisa ser
   calculada **uma vez**, num lugar que tanto a emissão quanto o lowering do
   call site leiam — o mesmo padrão de "computado uma vez, nunca discordam"
   que `constructor_index` já usa (leia o doc comment de
   `ir::Constructor::constructor_index`, `crates/server/src/ir/mod.rs:886`).

2. Os demais recebem o nome-ponte com sufixo de tipo, usando
   `dart_overload_name` — a mesma função que a passada de renomeação já usa:
   `addInt`, `subtractInt`, `lessThanDouble`.

3. `lower_record_operator_call` passa a consultar essa decisão em vez de mapear
   cegamente para `ir::Expr::Binary`: quando o overload resolvido pelo
   `libclang` (`clang_getCursorReferenced` do call cursor) **não** for o
   nativo, emitir `ir::Expr::Call` para o nome-ponte.

4. Remova `operator!=` da lista de operadores que geram declaração. Dart deriva
   `!=` de `==` automaticamente; declarar um método para ele é código morto que
   só serve para colidir. O call site de `a != b` já vira `ir::BinaryOp::Ne`
   corretamente hoje (`lower_record_operator_call`), então nada mais precisa
   mudar do lado da chamada. Confirme lendo o Dart emitido depois.

## Método

TDD, conforme `AGENTS.md`:

1. **Teste que falha primeiro** para a metade A (estilo
   `crates/server/tests/lower_cpp.rs` + `emit_dart.rs`):

   ```cpp
   #include <vector>
   class Linhas {
   public:
       int& operator[](int i) { return _v[i]; }
       int soma() const { return 0; }
   private:
       std::vector<int> _v;
   };
   int primeiro(Linhas &l) { return l[0]; }
   ```

   Verifique que o Dart de `primeiro` contém `l[0]` e **não**
   `l.unsupportedOperator(0)`.

2. **Teste que falha primeiro** para a metade B:

   ```cpp
   class Num {
   public:
       Num operator+(const Num &o) const;
       Num operator+(int v) const;
   };
   ```

   Verifique que o Dart emitido tem exatamente um `operator +` e um método
   nomeado distinto, e que uma chamada `a + 1` no C++ chega ao método nomeado,
   não ao operador.

3. **Teste do unário**: `Num operator-() const;` e um uso `-a`.

4. Implemente até passar. `just test` (ou `just test-host`, registrando),
   `just check`, `just lint`.

## Critério de sucesso

Depois de `just verovio-diagnosis`:

- `undefined_method` sobre `'unsupportedOperator'`: **1.101 → 0**. Verificação
  direta: `grep -rc "unsupportedOperator" .diagnosis/dart-package/lib/` deve
  cair drasticamente — o nome só deve sobrar para operadores que Dart
  genuinamente não tem (`->`, `,`) e mesmo esses agora com sufixo único.
- `duplicate_definition`: **44 → 0**.
- `undefined_method` total: **2.272 → abaixo de 1.200**.
- Nenhum `code` novo. `undefined_operator` (76 hoje) não pode subir.
- As três contagens de bailout não podem subir.

## Quando parar e perguntar

Só por decisão de **produto**, e este é um caso em que ela existe de verdade:
qual overload fica com a forma nativa do Dart é uma escolha visível para quem
lê o código gerado. A regra proposta ("o de mesmo tipo do receptor") é a mais
previsível, mas se o corpus tiver classes onde ela produz um resultado
estranho, traga o exemplo e a contagem.

Dificuldade técnica não é motivo para parar.
