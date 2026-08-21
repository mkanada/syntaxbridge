# Tarefa 09 — `Base::metodo()` virou recursão infinita (e o `dart analyze` não vê)

## Contexto do projeto

Syntax Bridge é uma IDE que transpila C/C++ para Dart. Servidor em Rust
(`crates/server/`), cliente Flutter. Leia `AGENTS.md` na raiz antes de começar —
ele é normativo (TDD obrigatório, `dynamic` proibido, silêncio proibido: uma
construção que não pode ser traduzida vira bailout explícito, nunca uma
tradução plausível e errada).

Use as receitas do `justfile`, não `cargo` cru. `just test` roda a suíte dentro
do Flatpak; `just test-host` roda na máquina quando o Flatpak não estiver
disponível (registre isso no resumo final).

Diagnóstico de origem: `docs/plans/dart-analyze-verovio-6.2.0.md`, família
**F12**. Este prompt é autocontido.

**Execute as tarefas 01 e 02 antes desta.** A 01 traz de volta os métodos
definidos out-of-line — que é onde mora a maioria dos overrides do Verovio, e
portanto a maioria das chamadas afetadas aqui. A 02 fixa a forma (`class` /
`mixin`) de cada registro, que muda como `super` resolve.

## A causa raiz

Dentro de um override, C++ chama a implementação da base com o nome
qualificado: `EditorialElement::Reset();`. O lowering perde a qualificação e
emite uma chamada ao próprio método — **recursão infinita**.

`super.` **não aparece uma única vez** nos 301 arquivos `.dart` emitidos do
Verovio 6.2.0. Verificação direta:

```
$ grep -rn "super\." .diagnosis/dart-package/lib/*.dart | wc -l
0
```

## A evidência

C++ original, `src/abbr.cpp:35-38`:

```cpp
void Abbr::Reset()
{
    EditorialElement::Reset();
    this->ResetSource();
}
```

Dart emitido, `.diagnosis/dart-package/lib/abbr.dart:28-32`:

```dart
@override
void Reset() {
  Reset();          // ← chama a si mesmo
  ResetSource();
}
```

`Abbr.Reset()` chama `Abbr.Reset()` até estourar a pilha.

**O `dart analyze` não reporta nada.** É Dart perfeitamente válido: 24.791
diagnósticos no relatório e nenhum deles é este. Por isso esta tarefa não tem
um `code` alvo — o critério de sucesso é outro (ver abaixo).

Uma varredura deliberadamente conservadora do pacote emitido (só o primeiro
statement de cada método, só assinaturas que cabem numa linha) encontra **61**
métodos que chamam a si mesmos incondicionalmente como primeiro statement,
quase todos `Reset` — `anchoredtext.dart:47`, `divline.dart:57`,
`liquescent.dart:84`, `chord.dart:83`, `keyaccid.dart:70`, `annot.dart:29`,
`f.dart:53`, `hairpin.dart:115`, `dynam.dart:85`, `layerdef.dart:31`,
`episema.dart:77`, `div.dart:39`…

O número real é maior (a varredura ignora chamadas que não são o primeiro
statement e assinaturas multilinha), e vai crescer muito depois da tarefa 01.

## O ponto difícil

Não basta emitir `super.Reset()`.

O pipeline traduz herança múltipla C++ achatando a ancestralidade numa lista de
mixins. Um exemplo real, `.diagnosis/dart-package/lib/abbr.dart:11`:

```dart
class Abbr with BoundingBox, VrvObject, VisibilityDrawingInterface,
    SystemMilestoneInterface, AttConverterBase, Att, AttLabelled, AttTyped,
    EditorialElement, AttSource { … }
```

Em Dart, `super.Reset()` resolve pela **linearização de mixins** — o último
mixin da lista que declara `Reset` vence. O C++ nomeou `EditorialElement`
explicitamente. Os dois só coincidem quando a base nomeada é a última da
linearização que tem aquele membro.

Quando não coincidirem, `super.Reset()` chamaria **outro método** — o que é
pior do que a recursão atual, porque parece funcionar. Nesse caso a saída certa
é um bailout explícito, não um `super.` que chama outra coisa.

## Onde mexer

- `crates/server/src/lower/cpp.rs` — o caminho que lowera uma chamada de método
  com receptor implícito. Um `CXCursor_CallExpr` cujo `referenced` é um
  `CXCursor_CXXMethod` de um registro **ancestral** do registro que contém a
  chamada, escrito na forma qualificada, é o padrão a reconhecer. `libclang`
  distingue a chamada qualificada da virtual — verifique com
  `clang -Xclang -ast-dump` num fixture pequeno antes de assumir a forma do
  cursor (o repositório já usa essa técnica; vários doc comments em `cpp.rs`
  registram achados obtidos assim).
- `crates/server/src/ir/mod.rs` — provavelmente uma variante nova de
  `ir::Expr` (ou um campo em `Expr::Call`) que diga "esta chamada é para a
  implementação da base, não para o despacho virtual". Sem isso o emissor não
  tem como distinguir.
- `crates/server/src/emit/dart.rs` — emitir `super.nome(args)` para essa
  variante, **depois de verificar** que a base nomeada é a que a linearização
  do Dart vai escolher. A informação necessária (a lista expandida de mixins do
  registro) já existe em `expand_mixin_chain` (~279).

## Método

TDD, conforme `AGENTS.md`:

1. Teste que falha primeiro, caso simples: herança **única**, `class B : A`,
   com `B::f()` chamando `A::f();`. Verifique que o Dart emitido é
   `super.f()` e não `f()`. Veja `crates/server/tests/lower_cpp.rs` para o
   padrão de fixture.
2. Teste do caso perigoso: herança múltipla onde a base nomeada **não** é a
   última da linearização que declara aquele membro. Verifique que o resultado
   é um bailout explícito, e **não** um `super.` que chamaria outro método.
3. Teste de não-regressão: uma chamada virtual normal (`this->f()` ou `f()` sem
   qualificação) continua sendo `f()`, não `super.f()`.
4. `just test` (ou `just test-host`, registrando no resumo), `just check`,
   `just lint`.

## Critério de sucesso

Esta tarefa **não** tem um `code` do `dart analyze` como alvo — o bug é
invisível para o analisador. Os critérios são:

1. **Zero métodos auto-recursivos incondicionais** no pacote emitido. Escreva
   um script descartável que varra `.diagnosis/dart-package/lib/*.dart`
   procurando métodos cujo corpo chama o próprio nome sem receptor e sem
   condição de parada, e verifique que a contagem cai de ≥61 para zero. (Se
   sobrar algum caso que seja recursão **legítima** do C++ original — existe
   código assim — documente cada um no resumo, com o arquivo e a linha do C++
   que o justifica.)
2. `grep -rn "super\." .diagnosis/dart-package/lib/*.dart` passa de **0** para
   um número na casa das centenas depois da tarefa 01.
3. Nenhum `code` novo no `dart analyze`. O risco específico:
   `abstract_super_member_reference` e `undefined_super_member`, que aparecem
   quando `super.x` aponta para algo que a linearização não oferece — se
   qualquer um dos dois surgir, a verificação de linearização está errada.
4. A contagem total de erros e avisos não pode subir.

Rode `just verovio-diagnosis` (5-6 min; `just package-build` antes se rodar no
Flatpak) para os itens 2-4.

## Quando parar e perguntar

Só por decisão de **produto**. O caso previsível: quando a base nomeada pelo
C++ não é alcançável por `super` na linearização do Dart, as saídas são
(a) bailout explícito, ou (b) chamar o método diretamente pelo mixin
(`EditorialElement.Reset` só é alcançável assim se ele for `static`, o que
mudaria a emissão daquele mixin inteiro), ou (c) reordenar a linearização de
mixins para que a base nomeada caia no lugar certo — o que só funciona se
houver uma ordem que satisfaça todas as chamadas do arquivo, e é decisão que
interage com a tarefa 02. Se (c) parecer viável no corpus real, pergunte antes
de implementar.

Dificuldade técnica não é motivo para parar.
