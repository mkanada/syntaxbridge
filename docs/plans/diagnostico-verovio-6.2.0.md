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

## Medição mais recente (2026-08-17)

A partir desta rodada, `just verovio-diagnosis` grava o snapshot bruto da
última execução em `.diagnosis/verovio-6.2.0.{json,md}` (gitignored,
sobrescrito a cada rodada — ver `crates/server/tests/verovio_6_2_0_transpile_diagnosis.rs`),
para consultar o estado atual sem reler este histórico nem rodar de novo
(~5min). Esta seção resume a rodada mais recente, com todos os achados
abaixo já aplicados até onde estão marcados como corrigidos/parcialmente
corrigidos, mais o achado 3 (`operator()`, commit `e849c51`):

| Métrica | Valor | Linha de base (primeira rodada) |
| --- | --- | --- |
| Unidades de compilação | 298 | 298 |
| Funções livres lowered | 513 | 513 |
| Classes/structs lowered | 1345 | 1345 |
| Arquivos `.dart` emitidos | 301 | 296 |
| Linhas emitidas | 63.821 | 677.708 |
| Linhas stub (expressão + statement) | 10.640 + 1.199 ≈ **18,6%** | 20.800 + 70.850 ≈ 13,5% |
| Arquivos que não parseiam | **16 / 301 (5%)** | 67 / 296 (23%) |
| Erros do `dart analyze` | **7.909** | 154.636 |
| Avisos do `dart analyze` | 10.191 | 19.754 |
| Tempo de extração `libclang` | 322,7s | ~300s |

A queda de 677.708 para 63.821 linhas emitidas não é (só) efeito dos achados
abaixo — o commit `76a5395` ("Corrige a emissao de enums e o custo
quadratico reintroduzido em emit_package", 2026-08-16, já mesclado antes
desta medição) corrigiu uma regressão de custo quadrático em
`emit_package` que multiplicava linhas emitidas; a primeira rodada deste
documento foi medida com essa regressão ainda presente, então a comparação
direta de "linhas emitidas" entre as duas colunas acima superestima o
efeito dos achados 1–6. A contagem de TUs/funções/records (vinda da
extração `libclang`, não do emissor) não muda entre as duas colunas — sinal
de que a extração nunca foi afetada, só a emissão.

`duplicate_definition` — o achado dominante da primeira rodada (85% dos
erros) — caiu para **514** nesta medição, batendo exatamente com o número
já registrado no achado 1 abaixo; não é mais o maior contribuinte. Ver
"Veredito" e "Recomendação" atualizados a seguir para o que domina agora.

## Veredito

O mecanismo central escala: as 298 unidades de compilação passam pela
extração sem travar. Na primeira rodada, cerca de 86% das linhas emitidas
eram lógica traduzida de verdade, não stub; na medição mais recente (ver
acima), esse número caiu para ~81% (18,6% stub) — não porque menos lógica
seja traduzida em termos absolutos, mas porque o achado 5 (ponteiro cru)
parou de dar bailout em assinaturas inteiras, expondo corpos de função reais
que antes ficavam escondidos atrás de um `Unsupported` único (mesma dinâmica
já descrita na seção do achado 5). O problema não é "o motor não aguenta
escala" — é que um punhado de construções C++ idiomáticas, nenhuma delas
exótica, nunca apareceu em nenhum dos treze degraus sintéticos e quebra a
saída em cascata.

**Na primeira rodada, 85% dos erros do `dart analyze` (132.023 de 154.636)
eram `duplicate_definition`**, e a causa raiz desse achado dominante foi
isolada com uma reprodução mínima, não é hipótese (achado 1 abaixo). Com os
achados 1 (parcial), 2, 3 (parcial) e 6 já aplicados, esse não é mais o
achado dominante: na medição mais recente, o maior contribuinte é
`receiver_of_type_never` (3.786 de 7.909, ~48%), seguido por `unused_field`
(2.096), `undefined_method` (2.066) e `dead_code` (1.993) — nenhum desses
quatro tem reprodução mínima isolada ainda (ver "Recomendação").

## Achados

### 1. Sobrecarga que não se distingue por tipo de parâmetro (causa de ~85% dos erros) — **parcialmente corrigido**

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

**Parcialmente corrigido (2026-08-17):** o caso `GetOffsetInterface()` —
ambos os lados sem parâmetro, diferindo só em constância — é exatamente
`mapping::overload_options_for`'s `"renomear-const-nao-const"`, já
corretamente reconhecido pelo solver (teste
`overload_options_for_recognizes_const_vs_non_const_even_with_a_covariant_return_type`
em `mapping.rs`, já verde antes desta correção). O gap estava um passo
depois: `function_catalog::apply_overload_renames` tratava esse id igual ao
`"renomear-por-tipo"`, chamando `dart_overload_name` (sufixo só por *tipo de
parâmetro*) para os dois lados — com zero parâmetros dos dois lados, o
sufixo saía vazio nos dois, e "renomear" produzia o mesmo nome duas vezes.
Corrigido: para `"renomear-const-nao-const"`, só o lado `const` ganha um
nome próprio (sufixo fixo `Const`, lido de `declaration.signature` via
`mapping::signature_is_const` — o mesmo texto que o solver já usa, extraído
para não haver duas fontes de verdade), o não-`const` mantém o nome
original, mesmo padrão que `"renomear-estatico-instancia"` já usa para o
par estático/instância. Prova:
`a_const_and_non_const_overload_with_no_parameters_get_distinct_dart_names`
em `crates/server/tests/function_catalog.rs`.

**Impacto medido no Verovio 6.2.0 real:** `duplicate_definition` caiu de 757
para 514 (−32%). **Não resolve o achado inteiro** — sobrevivem pelo menos
dois outros casos que o texto acima já previa e que este trabalho não
tocou: sobrecarga por aridade combinada com tipo de retorno diferente (ex.:
`Accid::IsAlignedWithSameLayer`, setter `void(bool)` vs. getter `bool()
const` — cai em `"parametro-opcional"`, que `apply_overload_renames`
deliberadamente não renomeia por decisão do E07, mas aqui os dois lados têm
*retorno* diferente, não é o mesmo membro com um parâmetro opcional) e
grupos com mais de dois membros. Nenhum dos dois tem reprodução mínima
isolada ainda.

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

### 3. `operator()` (padrão Functor) não é reconhecido — **parcialmente corrigido**

O Verovio usa pesadamente o padrão "Functor" (visitantes: `ResetDataFunctor`
e dezenas de variantes) via sobrecarga do operador de chamada,
`bool operator()(...)`. Nem `lower::cpp::lower_record_operator_call` (E13,
`+`/`-`/`*`/`==`/comparação) nem nenhuma outra parte do emissor reconhece
esse operador — o nome C++ (`operator()`) vaza literal para o Dart gerado,
que não aceita essa sintaxe (`operator()` não é um operador sobrecarregável
em Dart do jeito que `+`/`==`/`[]` são). Resultado: erro de *parse*, não só
de análise semântica — um dos motivos concretos por trás dos 23% de
arquivos que não formatam.

**Parcialmente corrigido** (commit `e849c51`, "Corrige declaracoes/chamadas
de operadores C++ sem forma direta em Dart"): regra geral por símbolo +
aridade, não caso especial por fixture. `operator()` vira o método `call` do
Dart; operadores no conjunto sobrecarregável do Dart (`+ - * / < <= > >= []
[]=`) viram declaração `operator <símbolo>` real; qualquer outro operador
(sem equivalente em Dart, ex. a maquinaria de `operator<=>`/`operator<` do
C++20) vira um método nomeado sintetizado com corpo em bailout explícito
(`Stmt::Unsupported`), nunca sintaxe quebrada nem omissão silenciosa. O
mesmo commit também corrigiu `--2147483647` (`-VRV_UNSET`, duplo unário):
`Expr::Unary` agora parenteiza o operando quando o texto começa com o mesmo
caractere do operador, evitando fusão no token de decremento do Dart.

**Impacto medido no Verovio 6.2.0 real, reportado no próprio commit:** 71/301
→ 52/301 arquivos inválidos (−27%), 9.599 → 8.361 erros do `dart analyze`
(−12,9%). Não marcado "corrigido" (só parcial) porque a regra "sem
equivalente em Dart vira bailout" ainda não teve seu próprio inventário de
quais operadores C++20 aparecem no Verovio além de `<=>`/`<` da maquinaria
de comparação.

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

### 6. `mixin X with Y, Z` / `mixin X extends Y` — sintaxe Dart inválida — **corrigido**

Achado novo (2026-08-17, não coberto pelos achados 1–5 acima). `emit_record`
(E09) sempre montava `extends_clause`/`with_clause` a partir de
`record.base_class`/`record.mixins` e só trocava a *keyword* (`class` →
`mixin`) quando o record também era usado como mixin em outro lugar
(`is_mixin`) — nunca suprimia essas duas cláusulas para esse caso. Dart
proíbe as duas em uma declaração `mixin` (só `on`, uma *constraint* de
superclasse, não composição, é permitida). Nunca apareceu em E01–E13: o
único fixture de herança múltipla (E09, `PatoDaguaVoador`) tem bases
(`Voador`/`Nadador`) sem base própria; o gap só aparece quando um record
usado como mixin *também* tem mais de uma base própria — real no Verovio
(`AltSymInterface : public Interface, public AttAltSym`, e este por sua vez
usado como mixin por `ControlElement`/`Note`/`Rest`).

**Correção, em três partes** (`crates/server/src/emit/dart.rs`):

1. Um record `is_mixin` agora emite `mixin M on A, B { ... }` (`base_class`
   e `mixins` combinados em uma única cláusula `on`) em vez de
   `extends`/`with`. Testes:
   `a_mixin_built_from_multiple_bases_uses_on_not_with_and_leaf_classes_expand_the_chain`,
   `a_mixin_with_a_single_base_uses_on_not_extends` em
   `crates/server/tests/emit_dart.rs`.
2. Como `on` é constraint, não composição, a classe *folha* que de fato
   aplica a cadeia via `with` precisa listar cada dependência transitiva
   por extenso, na ordem certa — `expand_mixin_chain`, nova função,
   substitui a impressão direta de `record.mixins`. Mesmo teste acima cobre
   o caso.
3. Duas lacunas expostas só ao medir contra o corpus real, não previstas
   pela reprodução mínima original:
   - `mixin_usrs` (decide quem ganha a keyword `mixin`) só olhava
     `record.mixins` diretos — um record alcançável só pelo `base_class` de
     *outro* mixin (ex.: `AttAltSym`'s próprio `Att`) nunca virava `mixin`,
     mesmo acabando em algum `with` via `expand_mixin_chain` — Dart's
     `mixin_of_non_class`. Corrigido fechando `mixin_usrs` sobre o mesmo
     grafo `base_class`/`mixins` que `expand_mixin_chain` percorre (fecho
     transitivo, não só a borda direta). Teste:
     `a_base_reachable_only_through_another_mixins_base_class_still_gets_the_mixin_keyword`.
   - A coleta de `import`s (`collect_referenced_usrs_in_record`, E11) só
     olhava os usrs *diretos* de `record.mixins` — a cláusula `with`
     expandida nomeia dependências transitivas que podem morar num arquivo
     nunca importado. Corrigido usando o mesmo `expand_mixin_chain` também
     aqui. Teste:
     `a_leaf_class_imports_every_transitively_expanded_mixin_dependency`.

**Impacto medido no Verovio 6.2.0 real:** arquivos que não parseiam como
Dart caíram de 52/301 (17%) para 16/301 (5%); `mixin_of_non_class` (novo,
introduzido pela primeira parte da correção e fechado pelas duas
seguintes) caiu de 929 para 4 — os 4 restantes são todos em `humlib.dart`
(`humlib`, biblioteca de terceiros embarcada no Verovio com estilo de
código bem diferente do resto do projeto — não investigado).

**Efeito colateral não resolvido, achado para uma próxima rodada:**
`extends_non_class` subiu de 244 (linha de base original) para 370. Causa
provável, ainda sem reprodução mínima isolada: um record pode ser alvo de
`extends` em um lugar (`class Y extends X`, `X` com uma única base própria)
e *também* aparecer em `mixins` de outro record em outro lugar — os dois
usos exigem keywords diferentes (`class` vs. `mixin`) para a *mesma*
declaração `X`, e o produto hoje só decide uma keyword por `usr`, global.
Nenhum dos treze degraus tem essa forma (um record com papel duplo); é o
mesmo tipo de tensão que motivou o achado 2 (nome sem namespace), mas na
dimensão "papel na herança", não "nome" — potencial achado 7.

### 8. Enum anônimo do C++ vira identificador Dart inválido

Achado novo (2026-08-17, encontrado na medição mais recente, entre os 16
arquivos que ainda não parseiam). `enum { PARTIAL_NONE, PARTIAL_THROUGH,
PARTIAL_RIGHT, PARTIAL_LEFT };` (C++ sem nome de tag, comum como campo
anônimo de struct) sai como:

```dart
enum (unnamed enum at .../vrv/beam.h:25:1) { PARTIAL_NONE, PARTIAL_THROUGH, PARTIAL_RIGHT, PARTIAL_LEFT }
```

O texto descritivo que o extrator usa internamente para identificar um enum
sem nome (`"(unnamed enum at <arquivo>:<linha>:<coluna>)"`, útil como chave
de diagnóstico/log) está vazando direto para a posição de identificador
Dart, em vez de virar um nome sintetizado válido (ex.: derivado do nome do
campo que o usa, ou um `AnonymousEnumN` determinístico por posição). Erro de
*parse*, mesma severidade do achado 3. Reprodução mínima: `include/vrv/beam.h:25`
no Verovio 6.2.0 real; `lib/beam.dart` é um dos 16 arquivos inválidos desta
rodada.

### 9. Parâmetro C++ sem nome quebra a assinatura Dart emitida

Achado novo (2026-08-17, mesma origem que o achado 8). Declarações C++ com
parâmetro sem nome (legal em C++, comum em assinaturas de interface/pura
virtual onde o parâmetro não é usado no corpo) saem com a vírgula/posição do
parâmetro mas sem identificador nenhum:

```dart
bool IsCloserToStaffThan(FloatingObject? , data_STAFFREL ) {
```

Dart exige um nome para cada parâmetro posicional. Erro de *parse*.
Reprodução mínima: `FloatingObject::IsCloserToStaffThan` no Verovio 6.2.0
real; `lib/floatingobject.dart` é um dos 16 arquivos inválidos desta rodada.
Correção provável: sintetizar um nome posicional (`arg0`, `arg1`, ...)
sempre que o parâmetro C++ não tiver `spelling`, mesmo padrão que outras
sínteses de nome já usam no projeto.

## O que já funciona

Nada do que os treze degraus construíram foi contradito por esta escala:

- As 298 unidades de compilação passam pela extração `libclang` real (corpos
  de função inclusos) sem travar, no mesmo tempo de ordem de grandeza já
  documentado para a passada de tipos (US-1: "~3min em 4 núcleos").
- A maior parte das linhas emitidas é lógica traduzida de verdade, não stub
  (~81% na medição mais recente, ver "Medição mais recente" acima) — herança
  múltipla, RAII, sobrecarga por tipo, biblioteca padrão limitada, tudo isso
  aparece corretamente no Verovio real, não só nos fixtures que os
  ensinaram.
- Nenhum achado aqui é "o extrator trava" ou "o emissor produz lixo em
  largura" — são lacunas pontuais e nomeáveis, cada uma isolável com uma
  reprodução mínima, exatamente o padrão que E01–E13 já estabeleceram.

## Recomendação

Por alavancagem, não por ordem de descoberta. Lista original preservada
abaixo (itens 1–7) como registro; itens 8–10 são a atualização pós-medição
mais recente (2026-08-17, ver seção acima) e são a ordem a seguir a partir
de agora.

**Estado atual dos itens originais:**

1. ~~Achado 1 (sobrecarga cega a tipo de parâmetro) primeiro~~ — **parcial**:
   caso const/não-const corrigido (item 7 abaixo permanece aberto).
2. ~~Achado 3 (`operator()`)~~ — **parcial**, ver seção do achado 3
   (commit `e849c51`).
3. **Achado 4 (STL não reconhecida vira tipo inválido) — ainda não feito.**
   Continua correção rápida e na linha de "silêncio é proibido": fazer
   `lower_type` cair em `Type::Unsupported` para qualquer especialização de
   template da stdlib sem adaptador, em vez de tratá-la como um `Record` do
   usuário.
4. **Achado 2 (nome sem namespace) — feito.**
5. **Achado 5 (ponteiro cru) — parcialmente feito.** Ver seção do achado 5;
   a contagem de erros subiu antes de poder cair, como já previsto ali.
6. **Achado 6 (`mixin ... with`/`extends` inválido) — feito.** Revelou o
   achado 7 candidato (`extends_non_class` 244 → 370), ainda sem
   reprodução mínima isolada.
7. **Achado 1, restante — ainda aberto.** Sobrecarga por aridade com
   retorno diferente (`IsAlignedWithSameLayer`) e grupos com 3+ membros.

**Próximos passos, por alavancagem na medição de 2026-08-17 (7.909 erros,
16/301 arquivos inválidos):**

8. **Achados 8 e 9 (enum anônimo, parâmetro sem nome) primeiro** — os dois
   são erros de *parse*, a categoria que a própria convenção deste
   documento já trata como mais grave que erro de análise (quebra até a
   formatação, não só `dart analyze`). Ambos têm reprodução mínima real no
   Verovio (`beam.h:25`, `FloatingObject::IsCloserToStaffThan`) e correção
   provável pequena e isolada (nome sintetizado em vez de texto
   descritivo/vazio). Juntos, dois dos 16 arquivos que ainda não parseiam
   — vale checar quantos dos 14 restantes compartilham a mesma causa antes
   de assumir que são 16 causas distintas.
9. **Novo achado dominante candidato: `receiver_of_type_never` (3.786,
   ~48% dos erros) — ainda sem reprodução mínima isolada.** Hipótese a
   verificar primeiro (não confirmada): mesma dinâmica do achado 5—
   `emit::dart::emit_body` tipa uma expressão como `Never` quando a
   compila a partir de um bailout, e código que antes ficava atrás de um
   `Unsupported` de função inteira agora expõe chamadas/acessos sobre esses
   valores `Never`. Se confirmado, resolver o achado 1 restante (item 7) e
   o achado 4 (item 3) deve reduzir `receiver_of_type_never` como efeito
   colateral, na mesma lógica já observada no achado 5 — então vale medir
   de novo depois de resolver 7 e 3 antes de investir numa correção
   dedicada aqui. `unused_field` (2.096) e `undefined_method` (2.066), os
   próximos dois maiores, ainda não têm hipótese.
10. **Achado 1, restante (item 7) e achado 4 (item 3) continuam a maior
    alavancagem entre o que já tem causa raiz conhecida** — com o achado 3
    parcialmente resolvido e o achado 6 resolvido, são os dois achados mais
    antigos deste documento ainda sem correção, e a hipótese do item 9
    acima é mais um motivo para priorizá-los.
