# Conversão guiada por exemplos

Este documento descreve uma **ordem de execução** alternativa para o roadmap:
em vez de completar cada capacidade em largura antes de passar à próxima,
atravessar o produto inteiro de ponta a ponta com um exemplo mínimo, e depois
engrossar esse caminho com exemplos progressivamente mais difíceis.

Ele não substitui `docs/plans/User Steps.md`. Aquele documento diz **o que** o
produto precisa fazer; este diz **em que ordem** construir, e qual evidência
concreta declara cada pedaço pronto.

## Índice

1. [O problema com a ordem atual](#1--o-problema-com-a-ordem-atual)
2. [A ideia](#2--a-ideia)
3. [Como se encaixa no roadmap](#3--como-se-encaixa-no-roadmap)
4. [Anatomia de um exemplo](#4--anatomia-de-um-exemplo)
5. [O que um exemplo prova](#5--o-que-um-exemplo-prova)
6. [A escada](#6--a-escada)
7. [O esqueleto mínimo (E01)](#7--o-esqueleto-mínimo-e01)
8. [Regras de disciplina](#8--regras-de-disciplina)
9. [Ambiente e infraestrutura](#9--ambiente-e-infraestrutura)
10. [Riscos](#10--riscos)
11. [Sequência de trabalho](#11--sequência-de-trabalho)
12. [Decisões em aberto](#12--decisões-em-aberto)

---

## 1 — O problema com a ordem atual

US-1 a US-5 estão prontos e entregaram um analisador de C++ competente:
unidades de compilação, arquivos fonte, catálogo de tipos, usos de tipos,
catálogo de funções e grafo de chamadas. Nada disso, porém, produziu **uma
única linha de Dart**. O produto ainda não fez, nem uma vez, aquilo que o
define.

Três consequências práticas disso:

- **As decisões mais caras seguem não validadas.** US-7 (mapeamento) e US-8
  (geração) carregam a maior parte das dúvidas do projeto — modelo
  intermediário, semântica de valor, herança múltipla, ponteiros. Enquanto
  nenhum Dart for gerado, essas dúvidas são discutidas em prosa, não
  resolvidas por evidência.
- **O modelo intermediário continua adiado.** As "Observações transversais" do
  `User Steps.md` já registram que ele precisa existir antes de US-8 e que a
  decisão de projetá-lo agora ou depois deve ser consciente. Sem um consumidor
  real, projetá-lo é adivinhação.
- **US-6 está no caminho crítico sem precisar estar.** Como escrito, US-10
  (equivalência comportamental) depende de US-6, que depende de KLEE e
  GoogleTest dentro do Flatpak — a dependência de infraestrutura mais pesada do
  roadmap. Isso empurra a primeira prova de que a conversão funciona para
  depois de um trabalho de ambiente considerável.

Nenhum desses pontos é argumento contra US-6 ou contra a análise já feita. São
argumentos contra deixar o *fim* do processo por último.

## 2 — A ideia

Manter no repositório uma **escada de exemplos de conversão**: projetos C++
minúsculos, versionados em texto puro, ordenados por dificuldade, cada um
acompanhado do Dart esperado e de casos de comportamento observável.

O trabalho de desenvolvimento passa a ser: *escolher o degrau mais baixo que
ainda falha e fazê-lo passar sem quebrar os anteriores*.

Isto é a regra de ouro do `AGENTS.md` levada a sério em escala de produto: o
corpus de exemplos **é** o teste que falha. Cada degrau novo entra no
repositório vermelho, e o trabalho termina quando fica verde.

O primeiro degrau é deliberadamente ridículo — uma função que soma dois
inteiros. Fazer *só isso* passar de ponta a ponta obriga a existir: uma
representação intermediária, um extrator de C++ para ela, um emissor dela para
Dart, uma rota que dispara a conversão, uma tela que mostra o resultado e um
mecanismo que prova que o Dart se comporta como o C++. É o produto inteiro, em
miniatura, funcionando. Tudo o que vem depois é engrossar cada uma dessas
peças, com um teste concreto e falhando por vez.

## 3 — Como se encaixa no roadmap

A escada não cria passos novos. Ela corta US-7 a US-10 em fatias verticais
finas, e cada degrau move o status de vários passos de uma vez:

| Passo | Como a escada o atinge |
| --- | --- |
| US-6 | Sai do caminho crítico. O oráculo dos primeiros degraus é escrito à mão; US-6 depois **substitui a escrita manual** por KLEE + gtest, sem mudar o formato do registro de comportamento. |
| US-7 | Começa pelo caso trivial (E03: struct → classe, opção única) e só encontra escolha real no E07 e no E09. As decisões nascem como dado versionado no exemplo, não como interação de UI. |
| US-8 | Nasce no E01 com um emissor de cinco construções, e cresce um degrau por vez. O modelo intermediário nasce junto, dimensionado pelo que o degrau exige. |
| US-9 | `dart analyze` sobre a saída de cada exemplo é critério de aceitação desde o E01. O Dart SDK já está no Flatpak (ver §9). |
| US-10 | O oráculo comportamental por exemplo é US-10 em escala pequena. O teste de mutação exigido pelo critério 3 de US-10 entra já no E01. |
| US-11 | Ganha um alvo concreto: o pacote exportado é o que o harness já compila fora do diretório do projeto. |
| US-12 | O par "antes/depois" que ele exige em fixture cai naturalmente como duas versões de um mesmo exemplo. |

Os degraus **não** substituem os critérios de aceitação de cada US. Um degrau
verde é evidência de que uma fatia daquele passo funciona; o passo só fica
`pronto` quando seus próprios critérios estiverem cobertos.

## 4 — Anatomia de um exemplo

Proposta de layout, em `examples/` na raiz do repositório (fora de
`test-resources/`, que hoje guarda archives binários; os exemplos precisam ser
texto puro para que o diff de um PR seja legível):

```
examples/
  E01-funcao-aritmetica/
    example.toml          # metadados do degrau
    input/                # projeto C++ completo e compilável
      CMakeLists.txt
      src/aritmetica.hpp
      src/aritmetica.cpp
    expected/             # Dart de referência (golden)
      lib/aritmetica.dart
    oracle/
      cases.json          # casos de comportamento observável
    decisions.toml        # decisões de US-7 fixadas (ausente até o E03)
    NOTES.md              # o que este degrau ensinou; armadilhas encontradas
```

`example.toml`:

```toml
id = "E01"
nome = "Função aritmética livre"
nivel = 1
status = "esperado-falhar"        # "passa" | "esperado-falhar"
motivo = "emissor Dart ainda não existe"
constroi = ["funcao-livre", "int", "expressao-binaria", "return"]
passos = ["US-7", "US-8", "US-9", "US-10"]
```

O campo `status` é o que mantém a escada honesta: degraus ainda não
implementados ficam versionados como `esperado-falhar`, e o harness **falha se
um exemplo marcado assim passar** — isso captura o caso em que algo começou a
funcionar por acidente e ninguém percebeu, que é tão ruim quanto uma regressão.

## 5 — O que um exemplo prova

Três critérios, em ordem crescente de dureza. Um degrau só fecha com os três.

### 5.1 — Gera (golden)

O Dart produzido é comparado com `expected/`. Este critério existe para tornar
a mudança **visível na revisão**: quando o emissor muda, o diff do PR mostra
exatamente como o código gerado mudou.

Ele é intencionalmente o critério mais fraco. Um golden sozinho congela o que
existe, inclusive os erros — uma suíte de goldens passa perfeitamente enquanto
gera Dart que não compila. Por isso o golden é **regravável** (`just
examples-bless`) e nunca é o único critério.

### 5.2 — Compila (`dart analyze`)

O pacote gerado passa em `dart analyze` sem erros e já está no formato de
`dart format`. É o critério 1 e 2 de US-9, aplicado exemplo a exemplo.

### 5.3 — Comporta (oráculo)

O critério que importa. Para cada caso em `oracle/cases.json`:

1. O harness gera um `main` C++ que chama a função com aqueles argumentos e
   imprime o resultado em formato canônico; compila com `clang++` usando as
   flags reais do `compile_commands.json` do exemplo e executa.
2. O harness gera um `main.dart` equivalente sobre o Dart **gerado** e executa
   com `dart run`.
3. Compara as duas saídas canônicas entre si.

O `expect` escrito à mão em `cases.json` é conferência de sanidade; **a verdade
é o C++ executado**, não o número que o autor do exemplo achou que sairia.
Formato:

```json
[
  { "chamada": "soma(2, 3)",  "espera": 5 },
  { "chamada": "soma(-1, 1)", "espera": 0 }
]
```

Este formato é o embrião do registro de comportamento de US-6. Ele precisa
nascer projetado para a comparação de US-10 — entradas, saída, efeitos —, e não
apenas para exibição, exatamente como US-6 já exige.

### 5.4 — E o teste de mutação

Uma suíte que só passa não prova nada. Junto do E01 entra um teste que
introduz uma divergência de propósito no emissor (trocar `+` por `-`) e exige
que o oráculo **falhe** com origem e valores esperado/obtido. É o critério 3 de
US-10, cobrado desde o primeiro degrau em vez de no fim.

## 6 — A escada

Cada degrau lista o que ele **força a existir** no produto e a **armadilha** —
o ponto em que a tradução deixa de ser textual. As armadilhas são o conteúdo
real da escada: os degraus são fáceis, o que se aprende ao subi-los não é.

| ID | Degrau | Força a existir | Armadilha |
| --- | --- | --- | --- |
| E01 | Função aritmética livre | IR mínima, extrator, emissor, rota, oráculo | `int` de 32 bits vs 64 bits |
| E02 | Controle de fluxo, laços, recursão | Statements, expressões, chamadas | Divisão inteira: `/` vira `~/` |
| E03 | `struct` POD | IR de agregado, ligação com US-3, decisão trivial de US-7 | Passagem por valor copia; em Dart, não |
| E04 | Classe com encapsulamento | Métodos, `this`, visibilidade, estáticos | `const` method e construtores múltiplos |
| E05 | `std::string`, `std::vector` | Adaptador de biblioteca padrão | `std::string` é bytes; `String` é UTF-16 |
| E06 | Herança simples e `virtual` | `extends`, `@override`, abstratas | Destrutor virtual não tem equivalente |
| E07 | Sobrecarga e parâmetros default | Decisão real de US-7, propagação a call sites | Renomear obriga a reescrever quem chama |
| E08 | Templates | Genéricos vs monomorfização | Especialização e SFINAE: recusar, não adivinhar |
| E09 | Herança múltipla | Opções com consequências, viabilidade global | Estado em mixin, ordem de linearização |
| E10 | Ponteiros, `union`, out params | Conceito de "código ponte", `dart:ffi` | Talvez a resposta certa seja recusar |
| E11 | Multi-TU, namespaces, CMake real | Estrutura de pacote, `pubspec.yaml`, dedup | Header incluído em N TUs duplica declarações |
| E12 | Exceções e RAII | `try`/`catch`/`throw`, destruição determinística | A conversão muda a forma do código do usuário |
| E13 | Fatia real do Verovio | Nada novo — prova que o resto vale fora do laboratório | Descobrir que valia só no laboratório |

### Detalhamento dos degraus decisivos

**E01 — Função aritmética livre.** `int soma(int a, int b) { return a + b; }`.
O objetivo não é o Dart produzido, é o esqueleto (§7).
*Armadilha:* o `int` do C++ tem 32 bits e o do Dart tem 64 na VM (e é `double`
na web). `soma(2, 3)` não expõe isso; `soma(2147483647, 1)` expõe. A v1 pode
declarar a premissa "ausência de overflow" — mas precisa **declarar**, com um
caso no oráculo marcado como divergência conhecida, não descobrir depois.

**E02 — Controle de fluxo.** `if`/`else`, `while`, `for` clássico, recursão,
`bool`, `double`.
*Armadilha:* `a / b` entre inteiros trunca em C++ e produz `double` em Dart —
precisa virar `~/`. É o primeiro ponto em que a tradução deixa de ser textual e
passa a exigir os **tipos** dos operandos. Se o emissor não tiver acesso a
tipos resolvidos, aqui ele descobre.

**E03 — `struct` POD.** `struct Ponto { double x, y; };` mais funções livres que
a recebem.
*Armadilha:* `void mover(Ponto p)` copia em C++ e passa referência em Dart. Um
caso de oráculo que muta `p` dentro da função e lê fora produz resultados
diferentes — silenciosamente, sem erro de compilação. Este é o primeiro degrau
em que o oráculo pega algo que nem o golden nem o `dart analyze` pegariam, e é
por isso que ele existe.

**E05 — Biblioteca padrão.** Primeiro degrau que exige separar *adaptador de
linguagem* de *adaptador de biblioteca*: `.size()` → `.length`, `push_back` →
`add`, `substr` → `substring` não são regras da linguagem, são de uma
biblioteca específica, e precisam morar em uma tabela substituível.
*Armadilha:* `std::string` indexa bytes; `String` do Dart indexa unidades UTF-16.
Só coincidem em ASCII. O oráculo deste degrau **precisa** conter um caso com
acento, para que a divergência apareça agora e não em um projeto real.

**E07 — Sobrecarga.** Duas funções com o mesmo nome não existem em Dart. As
opções (parâmetros opcionais, renomeação determinística) têm consequências
diferentes, e a escolha precisa ser persistida e sobreviver à reabertura do
projeto — é o critério 4 de US-7.
*Armadilha:* renomear `f(int)` para `fInt` obriga a reescrever **todos os call
sites**. É a primeira vez que uma decisão local altera código em outro arquivo,
e a primeira vez que o grafo de chamadas de US-5 é consumido como dado pelo
gerador, não apenas exibido na UI.

**E11 — Multi-TU.** Dois ou três arquivos, namespaces, headers compartilhados,
CMake de verdade.
*Armadilha:* um header incluído em três unidades de compilação produz a mesma
declaração três vezes nos catálogos. Os passes de US-3 a US-5 já convivem com
isso; um gerador não pode — emitir a mesma classe três vezes não compila. É
aqui que a "identidade estável de tipo" apontada em US-3 e cobrada em US-12
deixa de ser uma preocupação de projeto e vira um bug concreto.

**E13 — Degrau de realidade.** Uma classe pequena e autocontida extraída do
Verovio, escolhida por usar apenas construções dos degraus anteriores. O ponto
não é o tamanho: é descobrir se código escrito por gente real cabe nas regras
inventadas em cima de exemplos escritos por nós.
**Proposta:** um degrau de realidade a cada bloco — depois do E05, do E08 e do
E12 —, não apenas um no fim. Um degrau de realidade que falha vale mais do que
três degraus sintéticos que passam.

## 7 — O esqueleto mínimo (E01)

O que precisa existir para o primeiro degrau ficar verde. Cada peça nasce no
menor tamanho que resolve o E01 e cresce por degrau:

| Peça | Onde | Tamanho no E01 |
| --- | --- | --- |
| Modelo intermediário | `crates/server/src/ir/` | `Module`, `Function`, `Param`, `Type::Int`, `Block`, `Return`, `Binary`, `Ref` — e `Unsupported` |
| Extrator C++ → IR | `crates/server/src/lower/cpp.rs` | Visita o corpo de uma função e produz as construções acima |
| Emissor IR → Dart | `crates/server/src/emit/dart.rs` | Percorre a IR e escreve texto; ordenação estável, saída determinística |
| Orquestração | `crates/server/src/transpile.rs` | Lê os catálogos, chama extrator e emissor, grava o pacote |
| Rota | `crates/server/src/server.rs` | `POST /projects/transpile`, síncrona no início |
| UI | `client/flutter/lib/src/ui/source_file_viewer.dart` | Painel do Dart gerado ao lado do fonte C++ |
| Harness | `crates/server/tests/conversion_examples.rs` | Varre `examples/`, aplica os três critérios de §5 |
| Recipes | `justfile` | `just examples`, `just examples-bless` |

Duas observações de aproveitamento:

- **O extrator não precisa de uma quarta passada `libclang`.** O passe de US-5
  (`function_catalog::extract_function_catalog_cancellable`) já é o único que
  parseia corpos de função — os outros dois usam
  `CXTranslationUnit_SkipFunctionBodies` de propósito. A extração de IR é uma
  extensão daquele passe, não um passe novo, o que evita agravar o problema de
  escala já registrado em `User Steps.md`.
- **A rota pode nascer síncrona.** Transpilar o E01 leva milissegundos. Quando
  o custo aparecer (E11 ou E13), o mecanismo de job de `crates/server/src/jobs.rs`
  já existe, com progresso e cancelamento resolvidos, e é reaproveitado — como
  US-4 e US-5 já fizeram.

## 8 — Regras de disciplina

Sem estas regras, a escada degenera em um gerador que acerta os exemplos e erra
todo o resto.

1. **Nenhum caso especial por exemplo.** É proibido qualquer ramo no extrator
   ou no emissor que dependa de nome de arquivo, nome de função ou id de
   exemplo. Se um degrau só passa com um caso especial, a regra geral ainda não
   foi encontrada e o degrau não fechou.
2. **Todos os degraus anteriores continuam verdes.** Um degrau novo que quebra
   um antigo não está pronto — não importa quão mais difícil ele seja.
3. **Silêncio é proibido.** Toda construção C++ que a IR não representa vira um
   nó `Unsupported` com origem (arquivo, linha) e motivo. O emissor a
   transforma em falha explícita ou em `TODO` visível no Dart. Omitir código
   sem avisar é a única falha inaceitável, porque produz Dart que compila e
   está errado. É o critério 5 de US-8.
4. **O golden não é o contrato.** O contrato é `dart analyze` mais o oráculo. O
   golden é ferramenta de revisão, regravável — mas toda regravação aparece no
   diff do PR e precisa ser lida.
5. **Cada degrau fechado atualiza `User Steps.md`.** Status parcial dos passos
   tocados, e o que a armadilha ensinou. A escada alimenta o roadmap; não
   corre por fora dele.
6. **Cada degrau fechado deixa `NOTES.md` no exemplo.** Uma armadilha
   documentada no lugar onde ela mora vale mais que uma discussão perdida no
   histórico.

**Proposta em aberto:** exigir que todo degrau seja visível na UI para ser
considerado fechado. O argumento a favor é que "o usuário consegue ver o Dart"
é o único critério que impede a escada de virar um exercício de servidor. O
argumento contra é que degraus intermediários (E07, E09) mudam pouco na tela e
a exigência viraria cerimônia. Decidir antes do E03.

## 9 — Ambiente e infraestrutura

- **Dart SDK: disponível.** O manifesto Flatpak já traz o SDK 3.12.2
  (`build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`, módulo `dart-sdk`),
  instalado em `/app/lib/dart-sdk` com `/app/bin/dart` no caminho. US-9 e os
  critérios 5.2 e 5.3 desta escada são testáveis dentro do Flatpak hoje, e a
  versão está fixada por `sha256`, que é o que o critério de testabilidade de
  US-9 exige. (`User Steps.md` afirmava o contrário em três pontos; corrigido.)
- **`clang++` e `libclang`: disponíveis** via a extensão `llvm21`, já usada
  pelos passes existentes. O runner de oráculo do lado C++ não pede nada novo.
- **KLEE e GoogleTest: continuam fora do manifesto** — e, com o oráculo escrito
  à mão, saem do caminho crítico. Voltam a ser necessários em US-6, quando a
  geração automática de casos substituir a escrita manual. Essa substituição
  não deve mudar o formato de `oracle/cases.json`; se mudar, o formato foi mal
  projetado.
- **Execução de código de terceiros.** O oráculo compila e executa o código C++
  do exemplo. Nos exemplos deste corpus isso é inofensivo — são arquivos que
  nós mesmos escrevemos —, mas o mecanismo é o mesmo que US-6 usará sobre input
  arbitrário do usuário, e a posição de segurança exigida lá vale desde já:
  nada escreve fora do diretório do projeto.

## 10 — Riscos

- **Os exemplos são fáceis demais para significar alguma coisa.** É o risco
  principal. Mitigação: os degraus de realidade do §6, um a cada bloco, e não
  apenas no fim.
- **O gerador acerta os exemplos e erra o resto.** Mitigação: as regras 1 e 3
  do §8, mais o teste de mutação do §5.4. Um emissor que passa em tudo e não
  falha quando sabotado não está sendo testado.
- **Custo da suíte.** Cada exemplo compila C++ e roda Dart. Mitigação: manter
  os exemplos na casa dos segundos, cachear por hash do input, e marcar os
  degraus de realidade como `#[ignore]` — o precedente é
  `verovio_5_7_0_import_diagnosis.rs`.
- **Churn de golden.** Mudanças cosméticas no emissor reescrevem todos os
  goldens e afogam o diff. Mitigação: `just examples-bless` em commit separado
  do commit de comportamento.
- **A escada virar um roadmap paralelo.** Mitigação: a regra 5 do §8.

## 11 — Sequência de trabalho

Um PR por item. Os quatro primeiros são o esqueleto; do quinto em diante, um
degrau por PR.

1. **Infra do corpus.** `examples/`, `example.toml`, harness que varre o
   diretório e reporta cada exemplo como não implementado; `just examples`.
   Encerra vermelho, de propósito. É o teste que falha do `AGENTS.md`, em
   escala de produto.
2. **E01, caminho fino.** IR mínima, extrator, emissor, rota. Critérios 5.1 e
   5.2 passando.
3. **Oráculo comportamental.** Runner C++, runner Dart, comparação canônica,
   teste de mutação. Critério 5.3 passando. É aqui que o produto prova, pela
   primeira vez, que converteu de verdade.
4. **UI.** Painel de Dart gerado ao lado do fonte C++, reaproveitando o
   `source_file_viewer.dart` que já existe.
5. **E02 → E13**, um por PR, cada um fechando com `NOTES.md` e com a
   atualização do `User Steps.md`.

Depois do item 3, o `User Steps.md` deve passar a registrar US-8, US-9 e US-10
como `parcial` — com a fatia coberta escrita explicitamente, e não como
"parcial" genérico.

## 12 — Decisões em aberto

- **Local do corpus.** `examples/` na raiz (proposta) ou dentro de
  `test-resources/`. A favor da raiz: são material de leitura humana, não
  fixture binário.
- **Golden desde o E01 ou só quando a saída estabilizar.** Gerar golden antes
  do emissor amadurecer produz churn; não gerar deixa o E01 sem revisão
  visível.
- **Formato do oráculo.** `"chamada": "soma(2, 3)"` como texto exige um parser
  no harness; a alternativa estruturada (`{"funcao": "soma", "args": [2, 3]}`)
  é mais chata de ler e mais fácil de gerar automaticamente em US-6. A segunda
  provavelmente vence, mas a decisão pertence a quem escrever o item 3 do §11.
- **Onde vivem as decisões de US-7 dos exemplos.** `decisions.toml` por
  exemplo, aplicado ao banco antes de transpilar (proposta), respeitando o
  critério de testabilidade de US-7 de que decisões sejam expressáveis como
  dado, sem passar pela UI.
- **Premissa de overflow de inteiro.** Declarar `int` de C++ como equivalente a
  `int` de Dart e registrar a premissa, ou emitir mascaramento explícito para
  32 bits desde o E01. A primeira é mais simples e mais rápida de errar; a
  segunda polui todo o Dart gerado. Decidir no E01, por escrito.
- **Visibilidade na UI como critério de fechamento** (§8, proposta em aberto).
