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

## Medição mais recente (2026-08-17, após achados 8 e 9)

A partir desta rodada, `just verovio-diagnosis` grava o snapshot bruto da
última execução em `.diagnosis/verovio-6.2.0.{json,md}` (gitignored,
sobrescrito a cada rodada — ver `crates/server/tests/verovio_6_2_0_transpile_diagnosis.rs`),
para consultar o estado atual sem reler este histórico nem rodar de novo
(~5min). Esta seção resume a rodada mais recente — a segunda medição deste
mesmo dia, agora com os achados 8 e 9 (ver seções abaixo) também
corrigidos:

| Métrica | Valor | Medição anterior (mesmo dia) | Linha de base (primeira rodada) |
| --- | --- | --- | --- |
| Unidades de compilação | 298 | 298 | 298 |
| Funções livres lowered | 513 | 513 | 513 |
| Classes/structs lowered | 1345 | 1345 | 1345 |
| Arquivos `.dart` emitidos | 300 | 301 | 296 |
| Linhas emitidas | 63.756 | 63.821 | 677.708 |
| Linhas stub (expressão + statement) | 10.633 + 1.208 ≈ 18,6% | 10.640 + 1.199 ≈ 18,6% | 20.800 + 70.850 ≈ 13,5% |
| Arquivos que não parseiam | **7 / 300 (2%)** | 16 / 301 (5%) | 67 / 296 (23%) |
| Erros do `dart analyze` | **5.089** | 7.909 | 154.636 |
| Avisos do `dart analyze` | 10.190 | 10.191 | 19.754 |
| Tempo de extração `libclang` | 321,7s | 322,7s | ~300s |

Os achados 8 e 9 sozinhos derrubaram 9 dos 16 arquivos que ainda não
parseiam (16/301 → 7/300) e quase 36% dos erros do `dart analyze`
(7.909 → 5.089, −2.820) — bem mais que os dois arquivos de reprodução
mínima citados nas seções abaixo sugeririam: ambos os padrões (enum
anônimo, parâmetro sem nome) são comuns o bastante no Verovio para
aparecer espalhados por muitos arquivos diferentes, não só nos dois
citados como repro. Note que `not_enough_positional_arguments` (118) e
`extra_positional_arguments` (97) aparecem como regras novas nesta
medição — esperado: arquivos que antes nem chegavam a parsear (por causa
dos achados 8/9) agora expõem chamadas cujo número de argumentos o
`dart analyze` só consegue checar depois que a assinatura chamada
realmente parseia.

## Medição mais recente (2026-08-17, após os achados 10–12: zero arquivos inválidos)

Investigação do item 9 da Recomendação ("os 7 arquivos que ainda não
parseiam primeiro"): rodar `dart format --output=none` em cada um dos 7
arquivos (`.diagnosis/dart-package/`, instrumentação temporária de
`verovio_6_2_0_transpile_diagnosis.rs` que persiste o pacote emitido e
imprime o stderr completo de cada arquivo inválido, não só dos dois
primeiros) isolou três causas-raiz distintas, nenhuma delas prevista pelos
achados 1–9:

| Métrica | Valor | Medição anterior (mesmo dia) |
| --- | --- | --- |
| Unidades de compilação | 298 | 298 |
| Funções livres lowered | 515 | 515 |
| Classes/structs lowered | 1.344 | 1.345 |
| Arquivos `.dart` emitidos | 300 | 300 |
| Linhas emitidas | 63.707 | 63.766 |
| Linhas stub (expressão + statement) | 10.598 + 1.216 ≈ 18,5% | 10.633 + 1.210 ≈ 18,6% |
| Arquivos que não parseiam | **0 / 300 (0%)** | 7 / 300 (2%) |
| Erros do `dart analyze` | **4.991** | 5.090 |
| Avisos do `dart analyze` | 10.164 | 10.190 |
| Tempo de extração `libclang` | 328,3s | 323,9s |

**Todo arquivo emitido agora é, no mínimo, Dart sintaticamente válido** —
pela primeira vez desde que este diagnóstico existe, `arquivos que não
parseiam` chega a zero. A contagem de classes caiu em 1 (1.345 → 1.344,
esperada: o achado 11 abaixo faz um `struct` anônimo parar de ser
declarado, o comportamento correto) e a de erros do `dart analyze` caiu
99 (5.090 → 4.991, −1,9%) — um efeito pequeno perto dos achados 8/9,
porque os três achados desta rodada corrigem *erros de parse*
(`Could not format`), que nem chegavam a contar como erro de
`dart analyze` — eliminá-los destrava a *análise* de código que antes
ficava inteiramente invisível atrás de "o arquivo não parseia", não
elimina erros de análise por si só.

## Medição anterior (2026-08-18, diagnóstico estruturado e bailout de expressão)

Esta rodada trocou a leitura da saída humana de `dart analyze` por
`dart analyze --format=json`. Além da contagem por regra, o snapshot agora
preserva as primeiras ocorrências (regra, arquivo, linha, coluna e mensagem)
em `.diagnosis/verovio-6.2.0.{json,md}` e guarda a saída bruta em
`.diagnosis/verovio-6.2.0.analyze.json`. Isso transforma a próxima escolha em
evidência localizável, em vez de inferência a partir de texto de terminal.

| Métrica | Valor |
| --- | --- |
| Unidades de compilação | 298 |
| Funções livres lowered | 515 |
| Classes/structs lowered | 1.344 |
| Arquivos `.dart` emitidos | 300 |
| Linhas emitidas | 62.560 |
| Linhas stub (expressão + statement) | 9.980 + 1.361 ≈ 18,1% |
| Arquivos que não parseiam | **0 / 300 (0%)** |
| Erros do `dart analyze` | 4.163 |
| Avisos do `dart analyze` | 4.220 |
| Tempo de extração `libclang` | 413,8s |

O achado candidato `receiver_of_type_never` foi isolado com uma reprodução
mínima: uma expressão não suportada usada como receptor de acesso a campo. A
rodada descrita nesta seção usava temporariamente `dynamic` para evitar a
propagação de `Never`; a rodada seguinte substituiu esse paliativo por
`_syntaxBridgeUnsupported<T>`, que preserva o tipo estático esperado sem emitir
`dynamic` (ver a medição nova abaixo).

## Medição mais recente (2026-08-18, eliminação de `dynamic`)

O lowering agora normaliza os escalares fundamentais C/C++ para os escalares
mais próximos do Dart (`int` e `double`). Para uma expressão que não pode ser
traduzida, `Expr::UnsupportedTyped` preserva o tipo estático e o emissor gera
`_syntaxBridgeUnsupported<T>`. Tipos de fronteira ainda sem adaptador são
`SyntaxBridgeOpaque`, uma classe Dart nomeada; eles não são `dynamic`.

| Métrica | Valor |
| --- | --- |
| Unidades de compilação | 298 |
| Funções livres lowered | 515 |
| Classes/structs lowered | 1.344 |
| Arquivos `.dart` emitidos | 300 |
| Linhas emitidas | 60.284 |
| Linhas stub (expressão + statement) | 8.756 + 1.408 ≈ 16,9% |
| Arquivos que não parseiam | **0 / 300 (0%)** |
| Tokens `dynamic` emitidos | **0** |
| Linhas que referenciam `SyntaxBridgeOpaque` | 3.804 |
| Erros do `dart analyze` | 6.408 |
| Avisos do `dart analyze` | 4.048 |
| Tempo de extração `libclang` | 545,2s |

O aumento de erros em relação à medição anterior é intencionalmente
diagnóstico: operações que `dynamic` aceitava sem checagem agora revelam onde
faltam adaptadores específicos. As próximas pontes a priorizar são ponteiros e
buffers, produtos/coleções STL, callbacks e as fronteiras de stream/regex;
elas devem substituir `SyntaxBridgeOpaque` por contratos Dart nomeados.

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
achados 1 (parcial), 2, 3 (parcial), 6, 8 e 9 já aplicados, esse não é mais
o achado dominante — `duplicate_definition` caiu para 135 nesta medição
(era 514 na medição anterior do mesmo dia, antes dos achados 8/9; ver
"Recomendação" para a hipótese de por que corrigir arquivos que antes nem
parseavam também reduziu isso). O maior contribuinte na regra do
`dart analyze` (soma de erro + aviso por regra, não separado por
severidade) seguia sendo `receiver_of_type_never` (3.786). Ele foi eliminado
na rodada de 2026-08-18; o diagnóstico estruturado aponta agora
`undefined_method` (1.811 erros) como a principal lacuna semântica, seguido
por `extends_non_class` (370), `undefined_identifier` (359) e
`implicit_super_initializer_missing_arguments` (314). As primeiras ocorrências
convergem para a representação de herança múltipla como mixins (ver a
recomendação atualizada).

## Achados

### 1. Sobrecarga que não se distingue por tipo de parâmetro (causa de ~85% dos erros) — **parcialmente corrigido, avançado em 2026-08-17**

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
para 514 (−32%). **Não resolveu o achado inteiro** — sobreviveram pelo menos
dois outros casos que o texto acima já previa: sobrecarga por aridade
combinada com tipo de retorno diferente, e grupos com mais de dois membros.
Os dois avançaram nesta rodada (ver abaixo).

**Sub-caso "aridade + retorno diferente", corrigido (2026-08-17):**
reprodução mínima confirmada, `Accid::IsAlignedWithSameLayer` (setter
`void IsAlignedWithSameLayer(bool)` vs. getter
`bool IsAlignedWithSameLayer() const`) — `mapping::overload_options_for`
classificava qualquer grupo com aridades diferentes como
`"parametro-opcional"` (dobrar num único membro Dart com parâmetro
opcional), sem checar se os dois lados sequer concordam sobre o que
retornam. Aqui não concordam (`bool` vs. `void`) — não existe um único
membro Dart que possa declarar os dois retornos honestamente, então dobrar
é errado, precisa renomear. Corrigido: nova função `mapping::return_type_text`
extrai o texto do tipo de retorno de `FunctionDeclaration::signature`
(substring antes do nome qualificado — sempre nesse formato, não é um parser
de tipos); `overload_options_for` só escolhe `"parametro-opcional"` quando
as aridades diferem *e* todo o grupo concorda no tipo de retorno — caso
contrário cai no mesmo `"renomear-por-tipo"` que já existe para "mesma
aridade, tipos diferentes", cujo mecanismo de aplicação
(`function_catalog::apply_overload_renames` + `dart_overload_name`) já sabe
lidar com aridade zero de um lado (vira sufixo vazio, nome original
preservado, mesmo padrão do `"renomear-const-nao-const"`). Provas:
`overload_options_for_renames_instead_of_folding_when_arity_and_return_type_both_differ`
em `mapping.rs` (unitário) e
`a_getter_and_setter_pair_differing_in_both_arity_and_return_type_get_distinct_dart_names`
em `crates/server/tests/function_catalog.rs` (ponta a ponta, `libclang`
real).

**Sub-caso novo, descoberto só ao medir contra o corpus real, corrigido
(2026-08-17):** com o sub-caso acima corrigido, a medição revelou uma
segunda causa de `duplicate_definition` que o texto original do achado 1
não previa: duas sobrecargas distinguíveis *apenas* por um parâmetro que,
dos dois lados, é `Type::Unsupported` — mas com spellings C++ diferentes
(ex.: `int*` vs. `double*`, ambos caso C01, nunca ganham ponte).
`lower::cpp::overload_type_suffix` mapeava *qualquer* `Unsupported` para o
mesmo sufixo fixo `"Unsupported"`, descartando a única informação que
distinguiria os dois — `dart_overload_name` calculava o mesmo nome
renomeado para os dois lados, mesma classe de colisão, um passo mais fundo
que a decisão de renomear em si (que já estava correta: `"renomear-por-
tipo"`). Corrigido: o sufixo de um `Unsupported` agora incorpora o próprio
spelling C++, saneado por `pascal_case_alnum_segments` (nova função —
qualquer sequência de caracteres não-alfanuméricos é separador, cada
sequência alfanumérica vira PascalCase, junta sem separador — o mesmo
esquema que `function_catalog::pascal_case_namespace` já usa para `::`,
generalizado para a pontuação de um spelling de tipo C++ inteiro:
`*`/`<>`/`,`/espaço). Prova:
`two_overloads_distinguished_only_by_different_unsupported_parameter_types_get_distinct_dart_names`
em `crates/server/tests/function_catalog.rs` (ponta a ponta, repro mínima
`void Escrever(int*)` / `void Escrever(double*)`).

**Impacto medido no Verovio 6.2.0 real, os dois sub-casos juntos:**
`dart analyze` caiu de 4.227 para **4.162** erros (−65, −1,5%);
`duplicate_definition` caiu de 137 para **103** (−25%). Efeito modesto em
termos absolutos — os dois padrões são reais mas menos difundidos no
Verovio do que o caso const/não-const original (−32% sozinho). Os 103
`duplicate_definition` restantes já não têm reprodução mínima isolada, mas
a lista impressa pelo diagnóstico mostra pelo menos um exemplo claro de
"grupos com mais de dois membros" (`humlib.dart`'s `streamInsert`,
repetido três vezes) — ainda o gap que resta do achado 1, sem correção
tentada aqui.

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

### 4. Container STL não reconhecido vira referência de tipo inválida, não um `Unsupported` — **corrigido**

`std::string`/`std::vector`/`std::list`/`std::set`/`std::map` têm adaptador
(E05, mais os casos 4/5 de `docs/plans/verovio-6.2-pointer-types.md`);
qualquer outro template da stdlib (`std::array` confirmado como repro
mínima; `std::unordered_map`/`std::pair`/`std::optional`/etc. têm o mesmo
formato, não confirmados individualmente) não tem. A lacuna em si seria
honesta se virasse `Type::Unsupported` — mas `lower_type`'s ramo
`CXType_Record` não distinguia "template da stdlib sem adaptador" de "tipo
do próprio usuário": ambos viravam um `Type::Record` referenciando um nome
que nunca foi `lower_record`'d. Em Dart isso imprimia como uma referência de
tipo crua e indefinida (`array a` num parâmetro), sem nenhum marcador de
"isto não foi traduzido" na própria linha — o único jeito de descobrir era
ver o `dart analyze` reclamar de um tipo que não existe (`undefined_class`).
Pior que um stub honesto: parecia silencioso mesmo sem ser essa a intenção,
o tipo de divergência que a regra "silêncio é proibido" (`AGENTS.md`) existe
para evitar.

**Corrigido** (`crates/server/src/lower/cpp.rs`, `lower_type`'s
`CXType_Record`/`CXType_Unexposed` branch): o resultado de
`stdlib_template_name` (já calculado para reconhecer os cinco adaptadores)
agora é guardado numa variável e usado por completo — um `Some(other)` que
não bate com nenhum dos cinco nomes reconhecidos retorna `Type::Unsupported`
diretamente, em vez de cair no `_ => {}` que deixava o fluxo seguir até a
resolução genérica de usr/name (a mesma que trata um `Record` de verdade do
projeto). Só um template *nomeado sob o namespace `std`* entra nesse ramo
(`stdlib_template_name` só retorna `Some` nesse caso), então uma classe do
próprio usuário nunca é afetada. Prova:
`a_stdlib_container_without_an_adapter_becomes_unsupported_not_an_undeclared_record`
em `crates/server/tests/lower_cpp.rs` (`std::array<int, 3>` como repro
mínima confirmada).

**Impacto medido no Verovio 6.2.0 real:** erros do `dart analyze` caíram de
4.991 para **4.227** (−764, −15,3%), avisos de 10.164 para 9.904 (−260);
`undefined_class` (400 na medição anterior) some inteiramente do top 20.
Arquivos que não parseiam seguem em 0/300 (esse achado nunca produzia erro
de *parse* — `set campo` é Dart sintaticamente válido, só semanticamente
indefinido — então não afeta essa métrica). Linhas emitidas caem
ligeiramente (63.707 → 62.560): cada uso de um container sem adaptador
agora é uma linha de bailout explícita em vez de uma declaração de tipo
inválida que ainda "parecia" código real.

### 5. Ponteiro cru onipresente — **parcialmente corrigido**

E10 decidiu conscientemente não construir ponte para ponteiro cru
(`dart:ffi`), por falta de fixture que forçasse o custo
(`examples/E10-ponteiros-union-out-params/NOTES.md`). Em uma árvore de
objetos real com despacho virtual — exatamente a forma do Verovio — ponteiro
cru aparecia em uma fração enorme de campos, parâmetros e retornos, cada um
virando uma referência a `SyntaxBridgeOpaque`: sintaticamente válida e
explicitamente marcada como fronteira sem adaptador, mas ainda sem a segurança
semântica de uma ponte de ponteiro específica.

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

### 8. Enum anônimo do C++ vira identificador Dart inválido — **corrigido**

Achado novo (2026-08-17, encontrado na medição mais recente, entre os 16
arquivos que ainda não parseiam). `enum { PARTIAL_NONE, PARTIAL_THROUGH,
PARTIAL_RIGHT, PARTIAL_LEFT };` (C++ sem nome de tag, comum como campo
anônimo de struct) saía como:

```dart
enum (unnamed enum at .../vrv/beam.h:25:1) { PARTIAL_NONE, PARTIAL_THROUGH, PARTIAL_RIGHT, PARTIAL_LEFT }
```

O texto descritivo que este libclang usa para um `enum` sem nome
(`"(unnamed enum at <arquivo>:<linha>:<coluna>)"`) não é `""` como o código
assumia — `clang_getCursorSpelling` retorna esse texto explicativo em vez
de string vazia para um enum anônimo, então o guard `name.is_empty()` já
existente em `enum_identity`/`lower_type`'s `CXType_Enum` nunca disparava.
Erro de *parse*, mesma severidade do achado 3.

**Corrigido** (`crates/server/src/lower/cpp.rs`, `dart_enum_type_name`):
trocado o teste de vazio por `clang_Cursor_isAnonymous`, a API do libclang
que responde a pergunta certa independente de versão/wording — a mesma
função já é usada tanto por `enum_identity` (declaração) quanto pelo
branch `CXType_Enum` de `lower_type` (referência de tipo), então os dois
lugares corrigem juntos com uma única mudança. `enum_identity` também foi
simplificado para delegar inteiramente a `dart_enum_type_name` em vez de
duplicar a lógica de nome com uma checagem própria (que era, por sinal, a
checagem que não pegava o caso). Prova:
`an_anonymous_enum_is_never_declared_under_libclangs_debug_spelling` em
`crates/server/tests/lower_cpp.rs`. Reprodução mínima confirmada:
`include/vrv/beam.h:25` no Verovio 6.2.0 real.

**Impacto medido no Verovio 6.2.0 real (junto com o achado 9, não
separado):** ver "Medição mais recente" acima — os dois juntos derrubaram
16/301 → 7/300 arquivos inválidos e 7.909 → 5.089 erros do `dart analyze`.

### 9. Parâmetro C++ sem nome quebra a assinatura Dart emitida — **corrigido**

Achado novo (2026-08-17, mesma origem que o achado 8). Declarações C++ com
parâmetro sem nome (legal em C++, comum em assinaturas de interface/pura
virtual onde o parâmetro não é usado no corpo) saíam com a vírgula/posição
do parâmetro mas sem identificador nenhum:

```dart
bool IsCloserToStaffThan(FloatingObject? , data_STAFFREL ) {
```

Dart exige um nome para cada parâmetro posicional. Erro de *parse*.

**Corrigido** (`crates/server/src/lower/cpp.rs`,
`collect_params_with_clone_prelude` — a função única compartilhada por
função livre, método e construtor, `lower_function`/`lower_method`/o
construtor, todos afetados igualmente): quando `clang_getCursorSpelling`
do parâmetro vem vazio, sintetiza `arg{posição}` (`arg0`, `arg1`, ...) em
vez de propagar a string vazia. Prova:
`an_unnamed_parameter_gets_a_synthesized_positional_dart_name` em
`crates/server/tests/lower_cpp.rs`. Reprodução mínima confirmada:
`FloatingObject::IsCloserToStaffThan` no Verovio 6.2.0 real.

**Impacto medido no Verovio 6.2.0 real (junto com o achado 8, não
separado):** ver "Medição mais recente" acima. Isolar o efeito de cada um
separadamente exigiria duas rodadas adicionais (~5min cada); não feito
aqui porque os dois eram pequenos e isolados o bastante para valer aplicar
juntos direto — o combinado já mostra que os dois padrões, sozinhos,
respondem por 9 dos 16 arquivos inválidos e quase 36% dos erros da medição
anterior.

### 10. Identificador C++ colidindo com palavra reservada do Dart — **corrigido**

Achado novo (2026-08-17, investigação do item 9 da Recomendação: os 7
arquivos que ainda não parseiam após os achados 8/9).
`dart_enum_constant_name` (achado do E-degrau original, não desta rodada)
já tratava esse problema para *constantes de enum*, mas nenhum outro lugar
que cunha um identificador Dart a partir de um nome C++ aplicava a mesma
checagem — e um identificador C++ nomeado `is`, `in`, `var` ou `finally`
(nenhuma dessas é palavra reservada em C++) é comum o bastante em código
real para aparecer como nome de método (`bool is()`, `jsonxx.dart`), nome
de função livre, nome de parâmetro (`void f(int in)`, `vrv.dart`;
`void f(xpath_variable_boolean *var)`, `pugixml.dart`) e nome de variável
local (`basic_istringstream is = ...;`, `jsonxx.dart`) — cada um, sozinho,
um erro de *parse* (`'is' can't be used as an identifier because it's a
keyword`).

**Corrigido** com `lower::cpp::dart_safe_identifier`
(`crates/server/src/lower/cpp.rs`), uma função pura (sem tabela de
símbolos: dado o mesmo texto, sempre produz o mesmo resultado) que
generaliza a lista de palavras reservadas que `dart_enum_constant_name` já
tinha, agora compartilhada por todo lugar que cunha um identificador Dart
de valor. Dois mecanismos diferentes aplicam essa função, um para cada
classe de identificador:

- **Parâmetro e variável local** (com escopo léxico, não por `usr`):
  aplicada diretamente em `lower::cpp`, tanto na declaração
  (`collect_params_with_clone_prelude`, o `DeclStmt`/`VarDecl` de
  `lower_compound_stmt`) quanto em toda referência dentro do corpo
  (`dart_member_name`, já o único ponto de passagem tanto para campo
  quanto para parâmetro/local referenciado por `DeclRefExpr` — sendo pura,
  a mesma função nos dois lados garante que as duas nunca discordem, sem
  precisar de tabela alguma).
- **Método e função livre** (resolvidos por `usr` em cada chamada, não por
  nome): novo passe `function_catalog::apply_reserved_word_renames`, que
  reaproveita o mesmo mecanismo `renames: HashMap<usr, nome>` +
  `apply_renames` que `apply_overload_renames` (US-7) já estabeleceu para
  a sobrecarga — extraído para uma função compartilhada
  (`apply_renames`) para os dois passes de renomeação não poderem divergir
  em como uma renomeação é de fato aplicada. Roda logo depois do passe de
  sobrecarga (condições disjuntas, ordem entre os dois não importa).

Provas: `a_method_named_after_a_dart_reserved_word_is_renamed_at_declaration_and_call_site`,
`a_free_function_named_after_a_dart_reserved_word_is_renamed_at_declaration_and_call_site`,
`a_parameter_named_after_a_dart_reserved_word_gets_a_safe_dart_name`,
`a_local_variable_named_after_a_dart_reserved_word_gets_a_safe_dart_name`,
todos em `crates/server/tests/lower_cpp.rs`.

### 11. Struct anônimo vaza spelling de depuração do libclang — **corrigido**

Achado novo (2026-08-17, mesma investigação do achado 10; repro
confirmado: `zip_file.hpp:9461`, um `struct { ... } date_time;` sem nome de
tag dentro de outra classe). Mesmo padrão do achado 8 (enum anônimo), mas
para `struct`/`class`: `clang_getCursorSpelling` num record anônimo não
retorna vazio — retorna o texto de depuração
`"(unnamed struct at <arquivo>:<linha>:<coluna>)"` — e esse texto vazava
tanto para uma declaração `class (unnamed struct at ...) { ... }` (erro de
parse) quanto para o tipo de um campo desse struct (`(unnamed struct at
...) date_time;`, também erro de parse — mais grave que o achado 4, que
pelo menos produz um tipo sintaticamente válido, só inválido
semanticamente).

**Corrigido** (`crates/server/src/lower/cpp.rs`) com a mesma técnica do
achado 8: `clang_Cursor_isAnonymous`, não `name.is_empty()`, decide se um
record é anônimo. Dois pontos, espelhando exatamente `lower_record`/
`lower_type`'s `CXType_Enum` do achado 8:

- `lower_record` retorna `None` para um record anônimo (nunca declarado —
  não há nome Dart válido para declará-lo sob, mesmo raciocínio de
  `enum_identity`).
- `lower_type`'s ramo `CXType_Record` força `name` vazio para um `decl`
  anônimo, roteando pelo branch `Unsupported` já existente em vez de
  produzir um `Type::Record` apontando para uma classe nunca declarada —
um campo desse tipo vira a ponte explícita
  (`SyntaxBridgeOpaque /* unsupported: ... */`), o mesmo mecanismo que
  qualquer outro tipo ainda não representável usa.

Prova: `an_anonymous_struct_is_never_declared_under_libclangs_debug_spelling`
em `crates/server/tests/lower_cpp.rs`.

### 12. Alvo de `TupleAssign` atrás de receptor anulável quebra a sintaxe de pattern-assignment — **corrigido**

Achado novo (2026-08-17, mesma investigação; repro real:
`Fraction::ReduceStatic` do `iocmme.dart`, chamado com um campo alcançado
por um ponteiro anulável como argumento por referência —
`_m_mensInfo!.proportNum`). A ponte de out-param (E10/achado 5) emite uma
atribuição por desestruturação Dart, `(alvos...) = chamada;`. Quando um
alvo é um campo acessado por um receptor anulável, esse campo precisa de
`receptor!.campo` (o próprio `!` que o achado 5 já introduz) — mas a
gramática de *pattern assignment* do Dart não aceita um `!` pós-fixo
dentro de um elemento do padrão (`dart format`: "Expected to find ')'"
bem no `!`, confirmado empiricamente contra o arquivo real). Atribuição
comum (fora de um padrão) não tem essa restrição — `receptor!.campo =
valor;` é Dart válido — então o problema é específico da sintaxe de
desestruturação, não do `!` em si.

**Corrigido** (`crates/server/src/emit/dart.rs`): quando algum alvo de
`Stmt::TupleAssign` precisaria de `!` (`tuple_assign_needs_temp_block`,
testando se é um `FieldAccess`/`Index` cujo receptor é `Type::Nullable`),
a emissão contorna a gramática de padrão inteiramente — um bloco `{ ... }`
(escopo léxico próprio, evitando qualquer colisão de nome mesmo com uma
segunda chamada em ponte na mesma função) guarda o resultado da chamada
numa temporária de nome fixo, e cada alvo é atribuído individualmente com
sintaxe de atribuição comum (`receptor!.campo = temp.$N;`). Quando nenhum
alvo precisa de `!`, a sintaxe de pattern-assignment original é preservada
sem mudança.

Prova: `a_tuple_assign_target_reached_through_a_nullable_receiver_avoids_pattern_assignment_syntax`
em `crates/server/tests/lower_cpp.rs`.

**Impacto medido no Verovio 6.2.0 real (achados 10–12 juntos, não
separados — mesmo raciocínio dos achados 8/9: os três eram pequenos e
isolados o bastante para valer aplicar juntos):** ver "Medição mais
recente" acima. Os três achados juntos derrubaram os 7 arquivos que ainda
não parseavam para **zero** — a primeira vez que este diagnóstico mede
100% dos arquivos emitidos como Dart sintaticamente válido — e os erros do
`dart analyze` caíram 5.090 → 4.991 (−99, −1,9%, efeito modesto porque os
três corrigem erros de *parse*, que não contavam para essa métrica em
primeiro lugar).

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

1. ~~Achado 1 (sobrecarga cega a tipo de parâmetro) primeiro~~ — **parcial,
   avançado**: caso const/não-const, aridade+retorno diferente, e sufixo de
   `Unsupported` colidindo já corrigidos (item 12 abaixo — só "grupos com
   3+ membros" permanece aberto).
2. ~~Achado 3 (`operator()`)~~ — **parcial**, ver seção do achado 3
   (commit `e849c51`).
3. **Achado 4 (STL não reconhecida vira tipo inválido) — feito.** Ver
   seção do achado 4 e o item 11 da lista de alavancagem abaixo.
4. **Achado 2 (nome sem namespace) — feito.**
5. **Achado 5 (ponteiro cru) — parcialmente feito.** Ver seção do achado 5;
   a contagem de erros subiu antes de poder cair, como já previsto ali.
6. **Achado 6 (`mixin ... with`/`extends` inválido) — feito.** Revelou o
   achado 7 candidato (`extends_non_class` 244 → 370), ainda sem
   reprodução mínima isolada.
7. ~~Achado 1, restante~~ — **avançado (2026-08-17)**: sobrecarga por
   aridade com retorno diferente (`IsAlignedWithSameLayer`) e colisão de
   sufixo entre dois `Unsupported` de spelling diferente, ambos corrigidos
   — ver seção do achado 1. Só "grupos com 3+ membros" (item 12 abaixo)
   permanece aberto.

**Próximos passos, por alavancagem na medição de 2026-08-18 (4.163 erros,
0/300 arquivos inválidos):**

8. ~~Achados 8 e 9 (enum anônimo, parâmetro sem nome)~~ — **feitos**, ver
   seções acima. 16/301 → 7/300 arquivos inválidos, 7.909 → 5.089 erros.
9. ~~Os 7 arquivos que ainda não parseiam~~ — **feito**. Investigação
   isolou três causas-raiz independentes (achados 10–12 acima: identificador
   colidindo com palavra reservada do Dart, struct anônimo, alvo de
   `TupleAssign` atrás de receptor anulável) — nenhuma delas era
   `lib/humlib.dart`-específica (biblioteca de terceiros embarcada, a
   hipótese original para os 7): os três padrões apareciam espalhados por
   `jsonxx`/`tuningsimpl`/`vrv`/`pugixml`/`iocmme`/`zip_file` também. **0/300
   arquivos inválidos** na medição mais recente — pela primeira vez, todo
   arquivo emitido é Dart sintaticamente válido; ver "Medição mais
   recente" acima.
10. ~~`receiver_of_type_never`~~ — **feito (2026-08-18)**. A reprodução
    mínima confirmou que um bailout de expressão tipado como `Never` se
    propagava por acessos e chamadas posteriores. O helper agora é genérico
    (`_syntaxBridgeUnsupported<T>`) e preserva o tipo estático esperado; o
    pacote do Verovio não contém nenhum token `dynamic` emitido.
11. ~~Achado 4 (STL não reconhecida vira tipo inválido)~~ — **feito**. Ver
    seção do achado 4 acima: `undefined_class` (400) some do top 20; erros
    do `dart analyze` caem 4.991 → 4.227 (−15,3%).
12. ~~Achado 1, restante~~ — **avançado (2026-08-17), não fechado.**
    Sobrecarga por aridade com retorno diferente
    (`Accid::IsAlignedWithSameLayer`) e colisão de sufixo entre dois
    parâmetros `Unsupported` de spelling C++ diferente (`int*` vs.
    `double*`) — ambos corrigidos, ver seção do achado 1. Impacto medido:
    `dart analyze` 4.227 → 4.162 (−1,5%), `duplicate_definition` 137 → 103
    (−25%). **Ainda aberto:** grupos com 3+ membros (repro real no corpus:
    `humlib.dart`'s `streamInsert`, repetido três vezes), sem reprodução
    mínima isolada.
13. **Próximo alvo: contrato de herança de registros convertidos em `mixin`.**
    As ocorrências concretas conectam `undefined_method` (por exemplo,
    `LayerElement.GetAncestorStaff`), `override_on_non_overriding_member`,
    `extends_non_class`, `undefined_identifier` de membros herdados como
    `_m_doc`, e construtores de base obrigatórios. A hipótese verificável é
    que um registro C++ transformado em mixin deixa de expor, no tipo estático
    do próprio mixin, os membros recebidos pelos seus `on` constraints. Antes
    de mudar a estratégia de múltipla herança, criar uma reprodução mínima
    com base, mixin intermediário e chamada por referência ao mixin; comparar
    uma composição que preserve o contrato público e a inicialização de base.
