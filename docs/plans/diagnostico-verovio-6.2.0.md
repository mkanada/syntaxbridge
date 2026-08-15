# Diagnóstico: transpilação do Verovio 6.2.0 real

Avaliação de quão perto a transpilação (US-8, sobre a base construída por
E01–E13) está de lidar com um projeto C++ real e não modificado, ponta a
ponta — em vez de com os fixtures sintéticos da escada de exemplos
(`docs/plans/conversao-guiada-por-exemplos.md`) ou com a fatia deliberadamente
pequena do E13.

## Objetivo

Rodar `function_catalog::extract_function_catalog` + `emit::dart::emit_module`
sobre **todas** as unidades de compilação do Verovio 6.2.0 real
(`test-resources/verovio-version-6.2.0.tar.gz`, já usado por
`crates/server/tests/project_ingest.rs`) e avaliar o resultado — não como
critério de aceitação de um degrau, mas como sinal honesto de distância até o
alvo do produto.

## Metodologia

1. `ingest::create_project` sobre o tarball real → 298 unidades de compilação
   (`cmake` configura em ~1s).
2. `function_catalog::extract_function_catalog` sobre as 298 de uma vez —
   mesma passada paralela (um `CXIndex` por núcleo) que `POST /projects` já
   usa, com corpos de função reais (não `SkipFunctionBodies`).
3. `emit::dart::emit_module` direto sobre o `Module` resultante — **sem**
   passar cada arquivo por `dart format` como `transpile::transpile` faz.
   Essa chamada aborta o lote inteiro no primeiro arquivo que não formata, o
   que esconderia todo o resto atrás do primeiro erro; para um diagnóstico de
   cobertura, cada arquivo precisa do seu próprio veredito.
4. Escrita em um pacote Dart real (`pubspec.yaml` mínimo + `lib/*.dart`), e
   dois vereditos externos por arquivo: `dart format --output=none` (só
   parseia) e `dart analyze` sobre o pacote inteiro.
5. Métrica auxiliar, por contagem de linha (não um parser): fração das linhas
   emitidas que são um stub honesto (`_syntaxBridgeUnsupported(` em posição
   de expressão, `// TODO(syntax-bridge):` em posição de statement) — um
   proxy grosseiro, mas de ordem de grandeza útil, para "quanto do texto
   emitido é lógica de verdade, e não uma recusa explícita".

Implementado em `crates/server/tests/verovio_6_2_0_transpile_diagnosis.rs`
(`#[ignore]`, mesmo padrão de `verovio_5_7_0_import_diagnosis.rs` — não roda
em CI, é ferramenta de pesquisa, não critério de aceitação). Reproduzir com:

```
cargo test -p syntax-bridge-server --test verovio_6_2_0_transpile_diagnosis -- --ignored --nocapture
```

## Resultado agregado

| Métrica | Valor |
| --- | --- |
| Unidades de compilação | 298 |
| Funções livres lowered | 513 |
| Classes/structs lowered | 1345 |
| Arquivos `.dart` emitidos | 296 |
| Linhas emitidas | 677.708 |
| Linhas que são stub honesto (`Unsupported`) | 20.800 (expressão) + 70.850 (statement) ≈ **13,5%** |
| Arquivos que não parseiam como Dart (`dart format` falha) | **67 / 296 (23%)** |
| Erros do `dart analyze` sobre o pacote inteiro | **154.636** |
| Avisos do `dart analyze` | 19.754 |
| Tempo de extração `libclang` (4 núcleos) | ~300s |

Números da primeira rodada, antes de qualquer correção. Depois de corrigir o
achado 2 (ver abaixo), os erros do `dart analyze` caem para 154.632 — a
tabela acima fica como registro histórico do estado inicial, não é
atualizada a cada achado corrigido.

## Veredito

O mecanismo central escala: as 298 unidades de compilação passam pela
extração sem travar, e cerca de 86% das linhas emitidas são lógica traduzida
de verdade, não stub. O problema não é "o motor não aguenta escala" — é que
um punhado de construções C++ idiomáticas, nenhuma delas exótica, nunca
apareceu em nenhum dos treze degraus sintéticos e quebra a saída em cascata.
**85% dos erros do `dart analyze` (132.023 de 154.636) são
`duplicate_definition`**, e a causa raiz desse achado dominante foi isolada
com uma reprodução mínima, não é hipótese.

## Achados

### 1. Sobrecarga que não se distingue por tipo de parâmetro (causa de ~85% dos erros)

**Reprodução mínima:** rodando `extract_function_catalog` sobre **uma única**
unidade de compilação (`src/accid.cpp`, sem envolver as outras 297), a classe
`Accid` já sai com `GetOffsetInterface`, `GetPositionInterface`,
`GetDrawingUnisonAccid` e `IsAlignedWithSameLayer` — cada um com **dois**
`ir::Method` de `usr` distinto:

```
METHOD name="GetOffsetInterface" usr="c:@N@vrv@S@Accid@F@GetOffsetInterface#"
METHOD name="GetOffsetInterface" usr="c:@N@vrv@S@Accid@F@GetOffsetInterface#1"
```

Cada par é uma sobrecarga legítima em C++ (getter `const` e não-`const`, ou
uma variante por aridade — `bool IsAlignedWithSameLayer()` vs.
`void IsAlignedWithSameLayer(bool)`), do tipo que
`mapping::overload_options_for` já decide que precisa renomear
(`"renomear-por-tipo"`/`"renomear-const-nao-const"`). O problema está um
passo depois: `function_catalog::dart_overload_name` computa o sufixo do
nome renomeado **a partir do tipo dos parâmetros**. Quando os dois lados da
sobrecarga não têm parâmetro nenhum — como em `GetOffsetInterface()`, onde a
diferença está só na constância do método/retorno, não em um argumento — o
sufixo fica vazio dos dois lados, e "renomear" produz o mesmo nome duas
vezes. É a mesma classe de lacuna do achado 5 do E13
(`examples/E13-fatia-real-verovio/NOTES.md`: `Reduce` estático × `Reduce` de
instância, mesmo nome, zero parâmetros de um dos lados) — ali resolvida com
um paliativo específico (sufixo `Static` fixo), não com a correção geral:
"o esquema de sufixo por tipo de parâmetro é cego sempre que a distinção
entre as sobrecargas não mora na lista de parâmetros" (constância, tipo de
retorno, `static`-vs-instância, etc.).

Getter `const`/não-`const` é um padrão básico de C++ real, muito mais comum
em código de verdade do que em qualquer fixture sintético — é por isso que
esse achado nunca apareceu em treze degraus e domina a saída aqui.

### 2. Duas classes diferentes com o mesmo nome curto colidem — **corrigido**

Mesmo dentro de uma única unidade de compilação, duas classes de `usr`
diferentes chamadas `Object` coexistiam no catálogo. O emissor descartava o
namespace C++ ao nomear a classe Dart — gap já registrado em
`docs/plans/User Steps.md` (US-8, "Falta para pronto": "nome de `library`
Dart derivado de `namespace` C++, capturado desde o US-3/US-5, nunca usado
na geração") — e nunca combatido, porque nenhum fixture sintético tem duas
classes do próprio usuário com o mesmo nome curto em namespaces diferentes.

**Corrigido** com `function_catalog::apply_record_name_disambiguation`
(nova passada no mesmo ponto do pipeline de `apply_overload_renames`):
`ir::Record` ganhou o campo `namespace` (populado em `lower_record`, via
`type_catalog::namespace_of`); um record cujo `name` colide com o de outro
ganha o prefixo PascalCase do seu namespace (`ns1::Ponto` → `Ns1Ponto`), e o
que ainda colidir depois disso (mesmo namespace nos dois lados, ou nenhum
dos dois namespaced) ganha um sufixo numérico determinístico por ordem de
`usr` — um padrão simples, não necessariamente definitivo, mas sempre
único e determinístico. Todo lugar que referencia o tipo renomeado é
reescrito junto: campos, parâmetros, retornos, corpos de método/construtor,
`base_class`/`mixins`, `RecordConstruct`/`ConstructorCall`. Provado por
`two_records_with_the_same_name_in_different_namespaces_are_disambiguated`
em `crates/server/tests/lower_cpp.rs`.

**Limitação conhecida, não corrigida:** a referência sintética que
`lower::cpp::lower_static_method_call` cria para o receptor de uma chamada
de método estático (`Expr::Ref { name: nome_da_classe, ty: Type::Record
{...} }`, ver E13) carrega o nome da classe no campo `name` do `Ref`, não
só no `ty` — e `Ref.name` também é o campo usado para uma referência de
*variável local* comum, então a passada de renomeação não pode reescrevê-lo
de forma genérica sem risco de renomear uma variável que só *por
coincidência* tem tipo `Type::Record` com o `usr` renomeado. Não afeta
nenhum caso observado até aqui (nenhuma classe com nome colidindo tem
método estático chamado de fora dela nos fixtures ou no Verovio), mas é uma
lacuna real se essa combinação um dia aparecer.

**Impacto medido no Verovio 6.2.0 real:** de 154.636 para **154.632** erros
do `dart analyze` (132.023 → 132.021 `duplicate_definition`) — uma queda
pequena, como esperado: uma varredura estática do C++ real (antes de
implementar) já tinha contado só 3 pares de nome curto colidindo no projeto
inteiro (`PAEInput`, `Options`, `Object`). Este achado nunca foi o
dominante — é o achado 1 (sobrecarga cega a tipo de parâmetro) que responde
pelos 85% dos erros; ver "Recomendação" abaixo, que já classificava este
achado como de alavancagem baixa antes desta medição confirmar.

### 3. `operator()` (padrão Functor) não é reconhecido

O Verovio usa pesadamente o padrão "Functor" (visitantes: `ResetDataFunctor`
e dezenas de variantes) via sobrecarga do operador de chamada,
`bool operator()(...)`. Nem `lower::cpp::lower_record_operator_call` (E13,
`+`/`-`/`*`/`==`/comparação) nem nenhuma outra parte do emissor reconhece
esse operador — o nome C++ (`operator()`) vaza literal para o Dart gerado,
que não aceita essa sintaxe (`operator()` não é um operador sobrecarregável
em Dart do jeito que `+`/`==`/`[]` são). Resultado: erro de *parse*, não só
de análise semântica — um dos motivos concretos por trás dos 23% de
arquivos que não formatam.

### 4. Container STL não reconhecido vira referência de tipo inválida, não um `Unsupported`

`std::string`/`std::vector` têm adaptador (E05); qualquer outro container
(`std::set` confirmado; `std::map`/`std::array`/etc. têm o mesmo formato,
não confirmados individualmente) não tem. A lacuna em si seria honesta se
virasse `Type::Unsupported` — mas `lower_type`'s ramo `CXType_Record` não
distingue "template da stdlib sem adaptador" de "tipo do próprio usuário":
ambos viram um `Type::Record` referenciando um nome que nunca foi
`lower_record`'d. Em Dart isso imprime como uma referência de tipo crua e
indefinida (`set campo = 0;`), sem nenhum marcador de "isto não foi
traduzido" na própria linha — o único jeito de descobrir é ver o
`dart analyze` reclamar de um tipo que não existe. É pior que um stub
honesto: parece silencioso mesmo sem ser essa a intenção, o tipo de
divergência que a regra "silêncio é proibido" (`AGENTS.md`) existe para
evitar.

### 5. Ponteiro cru onipresente — **parcialmente corrigido**

E10 decidiu conscientemente não construir ponte para ponteiro cru
(`dart:ffi`), por falta de fixture que forçasse o custo
(`examples/E10-ponteiros-union-out-params/NOTES.md`). Em uma árvore de
objetos real com despacho virtual — exatamente a forma do Verovio — ponteiro
cru aparecia em uma fração enorme de campos, parâmetros e retornos, cada um
virando `dynamic /* unsupported: T * */`: sintaticamente válido, mas sem
nenhuma segurança de tipo.

**Parcialmente corrigido** com um solver dedicado,
`mapping::pointer_options_for` (caso A10, `docs/mapping-solver-cases.md`).
O resultado do solver não é uma classificação binária "representável ou
não" — é a lista real e finita de tipos concretos que o ponteiro pode
assumir em tempo de execução, calculada caminhando pelas mesmas arestas de
herança que já sustentam a decisão de herança múltipla do E09
(`possible_pointee_types`, na direção oposta: de uma base para baixo, até
toda subclasse dela no projeto). Essa lista é sempre finita porque o
próprio código fonte C++ é finito. Quando `T` já é um tipo que este projeto
representa por inteiro (`Record` do próprio projeto, ou `Str`/`List` do
adaptador do E05), a lista — mesmo no caso degenerado de uma única entrada,
`T` sem subclasses — é exatamente o que já torna a polimorfia de referência
única do Dart correta sem precisar enumerar subtipos no texto gerado:
`lower::cpp::lower_type` consulta o solver de verdade (sem o catálogo do
projeto em mãos, recebe a versão não enriquecida — ainda uma lista
genuína, só sem os subtipos) e mapeia `T*` direto para `T?`
(`ir::Type::Nullable`, novo). Quando `T` não é representável (`void`, um
escalar, ou algo que a própria análise já recusou), nada garante que o
ponteiro não seja usado como buffer/aritmética — continua `Unsupported`,
igual a C01, honesto sobre precisar de `dart:ffi`.

O gap em si não era uma descoberta nova; o que este diagnóstico acrescentou
foi a escala do custo — e a maior parte desse custo, em C++ orientado a
objetos idiomático como o Verovio, é exatamente a forma que o solver agora
resolve (ponteiro-para-classe usado como referência opcional), não a forma
de C01 (aritmética de ponteiro sobre escalar), que continua precisando de
ponte real.

**Efeito colateral corrigido antes de medir:** um `T*` virando `T?` sem mais
nada quebra `dart analyze` de um jeito novo — C++ nunca exige checar null
para desreferenciar um ponteiro (`p->x` compila, existindo ou não o objeto),
mas Dart exige um `!`/checagem antes de acessar campo/método por uma
referência anulável. Sem isso, o primeiro `dart analyze` real sobre o
Verovio *piorou* (154.632 → 158.110 erros): `unchecked_use_of_nullable_value`
apareceu 1.031 vezes. Corrigido emitindo `!` em todo acesso de
campo/método/índice cujo receptor seja `Type::Nullable`
(`emit::dart::receiver_bang`) — a mesma aposta que C++ já fazia
implicitamente (confiar na fonte, estourar em tempo de execução se errado,
não corromper estado em silêncio), só tornada explícita porque Dart exige
que apareça no texto.

**Impacto real medido no Verovio 6.2.0, depois do `!`:** 157.475 erros —
ainda **acima** da linha de base de 154.632 antes do solver de ponteiros, e
essa é a métrica bruta certa a reportar, não uma mais favorável. A causa não
é o solver estar errado — é a mesma dinâmica de "corrigir uma coisa revela
outra" que já apareceu ao fechar o E13: `emit::dart::emit_body` faz toda a
função sair como um `throw` único e opaco assim que *qualquer* parte da
assinatura (parâmetro, retorno) é `Type::Unsupported` — e um parâmetro/
retorno `T*` era isso antes deste solver. Com o parâmetro agora
`Type::Nullable`, a função deixa de sair inteira como stub e passa a emitir
o corpo de verdade pela primeira vez — o que expõe outras lacunas dentro
desse corpo que o bailout escondia (contagem de linhas confirma: stubs de
*expressão* quase dobraram, de 20.800 para 39.053, enquanto stubs de
*statement* — o sinal de bailout de função inteira — quase metade, de
70.850 para 50.710). `dead_code`/`receiver_of_type_never`/`undefined_method`
subiram (mais corpo real analisado, mais chance de bater em outra lacuna já
catalogada, sobretudo o achado 1); `unchecked_use_of_nullable_value` saiu
da lista; um novo aviso pequeno e inofensivo, `unnecessary_non_null_assertion`
(623, warning, não erro), é o custo de `receiver_bang` não fazer análise de
fluxo — insere `!` sempre que o tipo é anulável, mesmo quando o Dart já
provaria o valor não-nulo por outro caminho.

Em outras palavras: o solver está correto e é uma correção real de
tipagem — a contagem de erros bruta simplesmente não é a métrica certa para
julgá-lo isoladamente, porque desmascarar bailouts de função inteira é, por
natureza, uma operação que aumenta a contagem de erros visíveis antes de
poder diminuí-la (o mesmo corpo agora exposto só some da lista de erros
quando as lacunas que ele revela — sobretudo o achado 1 — também forem
corrigidas).

## O que já funciona

Nada do que os treze degraus construíram foi contradito por esta escala:

- As 298 unidades de compilação passam pela extração `libclang` real (corpos
  de função inclusos) sem travar, no mesmo tempo de ordem de grandeza já
  documentado para a passada de tipos (US-1: "~3min em 4 núcleos").
- ~86% das 677.708 linhas emitidas são lógica traduzida de verdade — herança
  múltipla, RAII, sobrecarga por tipo, biblioteca padrão limitada, tudo isso
  aparece corretamente no Verovio real, não só nos fixtures que os
  ensinaram.
- Nenhum achado aqui é "o extrator trava" ou "o emissor produz lixo em
  largura" — são lacunas pontuais e nomeáveis, cada uma isolável com uma
  reprodução mínima, exatamente o padrão que E01–E13 já estabeleceram.

## Recomendação

Por alavancagem, não por ordem de descoberta:

1. **Achado 1 (sobrecarga cega a tipo de parâmetro)** primeiro — sozinho
   responde por 85% dos erros de análise. Resolver o caso geral (distinguir
   por constância/aridade/tipo de retorno quando o tipo de parâmetro não
   basta), não outro paliativo pontual como o `Static` do E13.
2. **Achado 3 (`operator()`)** — pequeno e isolado, mas é uma fonte real de
   erro de *parse* (mais grave que erro de análise: quebra até a
   formatação), e o padrão Functor é estrutural no Verovio.
3. **Achado 4 (STL não reconhecida vira tipo inválido)** — correção rápida e
   na linha de "silêncio é proibido": fazer `lower_type` cair em
   `Type::Unsupported` para qualquer especialização de template da stdlib
   sem adaptador, em vez de tratá-la como um `Record` do usuário.
4. **Achado 2 (nome sem namespace) — feito.** Implementado com um padrão
   simples e determinístico (prefixo de namespace + sufixo numérico de
   desempate); impacto real medido foi pequeno (achado nunca foi
   dominante), mas fecha a lacuna e deixa o padrão de nomes aberto para
   revisão futura, se um caso real pedir algo mais refinado.
5. **Achado 5 (ponteiro cru) — parcialmente feito, com ressalva.** A fatia
   que dominava o custo em código orientado a objetos real
   (ponteiro-para-classe como referência opcional) tem solver e está
   resolvida de fato (`Type::Nullable`), não só descrita — mas a contagem
   bruta de erros do `dart analyze` subiu (154.632 → 157.475), porque
   deixar de dar bailout na assinatura de uma função inteira (o que um `T*`
   `Unsupported` fazia) expõe o corpo real dela pela primeira vez, e junto
   dele qualquer outra lacuna que já existisse ali — sobretudo o achado 1,
   ainda não corrigido. Sinal de que **corrigir o achado 1 agora vale mais
   ainda** do que antes: parte do aumento aqui deve se desfazer sozinho
   quando ele for resolvido. A fatia C01 (ponteiro-para-escalar com
   aritmética) continua precisando de ponte real via `dart:ffi` — trabalho
   maior do roteiro, não uma correção pontual, e ainda não construído.
