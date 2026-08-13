# Primeiro corte vertical — do C++ ao Dart (E01 → E03)

Documento de trabalho para quem for implementar. É autocontido: descreve o
objetivo, o que já existe, o que está dentro e fora do escopo, as decisões já
tomadas (que **não** devem ser relitigadas) e o critério verificável de
completude de cada entrega.

Complementa, sem substituir:

- `docs/plans/User Steps.md` — o roadmap por passo de usuário (US-1 a US-12).
- `docs/plans/conversao-guiada-por-exemplos.md` — a escada de exemplos (E01 a
  E13) e as regras de disciplina.
- `AGENTS.md` — método de desenvolvimento (TDD obrigatório) e comandos.

---

## 1 — Objetivo

Fazer o Syntax Bridge produzir, pela primeira vez, **Dart que comprovadamente
se comporta como o C++ de origem**, para os três degraus mais baixos da escada:

| Degrau | Conteúdo |
| --- | --- |
| E01 | Função livre aritmética: `int soma(int a, int b) { return a + b; }` |
| E02 | Controle de fluxo: `if`/`else`, `while`, `for` clássico, recursão, `bool`, `double` |
| E03 | `struct` POD (`struct Ponto { double x, y; };`) e funções livres que a recebem e devolvem |

"Comprovadamente" tem definição operacional, e é a razão de existir deste
documento: cada degrau só fecha com os três critérios do §5 da escada — golden,
`dart analyze` e **oráculo comportamental** (compilar o C++, executar o Dart
gerado, comparar as saídas). Um golden verde sozinho não prova nada; ele
congela inclusive os erros.

O objetivo **não** é um transpilador completo. É o esqueleto vertical do
produto, no menor tamanho que atravessa da ingestão (já pronta) até Dart
executável, com prova de equivalência. Tudo depois disso é engrossar cada peça
um degrau por vez.

---

## 2 — Onde o produto está hoje

US-1 a US-5 estão prontos e testados. O servidor Rust em `crates/server` já
entrega:

| Módulo | O que dá |
| --- | --- |
| `ingest.rs` | Descompacta o input, roda `cmake` com `CMAKE_EXPORT_COMPILE_COMMANDS`, lista as unidades de compilação |
| `source_catalog.rs` | Arquivos fonte do projeto (TUs + headers locais alcançados) |
| `type_catalog.rs` | Tipos declarados, com `usr` do `libclang` como identidade estável, grafo `type_dependencies` e usos de tipo |
| `function_catalog.rs` | Funções/métodos/construtores/destrutores/macros-função/templates, com assinatura, e o grafo de chamadas (`CallEdge`) |
| `persistence/project_store.rs` | Todas as tabelas no `project.db` de cada projeto |
| `project_service.rs` | Orquestração e os `*Listing` que a UI consome |
| `server.rs` | Rotas HTTP (axum) |
| `jobs.rs` / `progress.rs` | Trabalho longo: progresso por contadores atômicos e cancelamento por `AtomicBool` |

Cliente Flutter em `client/flutter`, com os painéis Source Files, Types,
Usages, Functions e Callers.

**Nada disso gerou uma única linha de Dart.** Não existem hoje os diretórios
`crates/server/src/ir/`, `lower/`, `emit/`, nem `transpile.rs`, nem `examples/`
na raiz. O primeiro corte é código novo em diretório novo; o único ponto de
contato com o que existe é estender `function_catalog.rs`.

### Ambiente

- **Dart SDK 3.12.2** já está no manifesto Flatpak
  (`build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`, módulo `dart-sdk`),
  em `/app/lib/dart-sdk`, com `/app/bin/dart` no caminho, versão fixada por
  `sha256`. `dart analyze`, `dart format` e `dart run` são utilizáveis hoje.
- **`clang++` e `libclang`** vêm da extensão `llvm21`, já usada pelos passes
  existentes.
- **KLEE não está** no manifesto, e não é necessário aqui — o oráculo deste
  corte é escrito à mão. (GoogleTest entrou no manifesto em 2026-08-13, ver
  AGENTS.md, mas também não é necessário aqui pela mesma razão.)

### Comandos

Use as receitas do `justfile`, **nunca** `cargo`/`flutter`/`dart` crus:
`just test` (dentro do Flatpak, é a suíte preferida), `just test-host` (na
máquina de desenvolvimento, quando o Flatpak não estiver disponível),
`just check`, `just lint`, `just fmt-check`, `just ci`. Se rodar fora do
Flatpak, registre isso no resumo final.

---

## 3 — Escopo

### Dentro

1. IR mínima em `crates/server/src/ir/`, dimensionada pelo que E01–E03 exigem.
2. *Lowering* C++ → IR, como **extensão** do passe de `function_catalog.rs`.
3. Emissor IR → Dart, determinístico.
4. Orquestração (`transpile.rs`) e rota `POST /projects/transpile`, **síncrona**.
5. Fatia mínima de US-7: opção única para `struct` sem herança múltipla,
   persistida, mais opção-ponte com motivo para todo o resto.
6. Corpus `examples/` com E01, E02 e E03, e o harness que os avalia.
7. Oráculo comportamental (runner C++, runner Dart, comparação canônica) e o
   teste de mutação.
8. Painel de Dart gerado na UI.

### Fora — e por quê

| Fora | Motivo |
| --- | --- |
| US-6 inteiro (caracterização) | Já decidido opcional de ponta a ponta em `AGENTS.md`; o oráculo destes degraus é escrito à mão |
| Solver de viabilidade global de US-7 (Q9) | O item mais caro do roadmap. Nestes três degraus não existe escolha a fazer: a lista de opções tem um elemento. Ele é dimensionado pelo E09 (herança múltipla), não aqui |
| Jobs/progresso/cancelamento na transpilação | Transpilar estes exemplos leva milissegundos. Quando o custo aparecer (E11/E13), `jobs.rs` já existe e é reaproveitado |
| Classes, herança, `virtual`, `std::string`, `std::vector`, templates, sobrecarga, ponteiros, exceções | São E04 a E12. Cada um vira um PR próprio depois |
| US-11 (exportação) e US-12 (re-ingestão) | Não são pré-requisito de ver Dart |
| Multi-TU e dedup de header incluído em N TUs | É a armadilha do E11. Cada exemplo deste corte tem uma única unidade de tradução |

---

## 4 — Decisões já tomadas

Estas decisões estão fechadas. Implemente-as; não as reabra. Se alguma se
mostrar errada **na prática, com evidência de teste**, registre em `NOTES.md`
do exemplo e escreva a nova decisão — mas não troque por preferência.

1. **Alvo do primeiro corte: até o E03**, inclusive. Não pare no E01.

2. **O corpus `examples/` existe desde o primeiro PR**, mesmo antes de haver
   emissor. Os três exemplos entram versionados como `status =
   "esperado-falhar"`, e o harness **falha se um exemplo marcado assim passar**
   — isso captura o caso em que algo começou a funcionar por acidente.

3. **Premissa de overflow de inteiro: declarada, não mascarada.** `int` de C++
   é emitido como `int` de Dart, sem mascaramento de 32 bits. A premissa é
   registrada como um caso de oráculo marcado como divergência conhecida (ver
   §6.3). Mascarar poluiria todo o Dart gerado; a alternativa foi rejeitada.

4. **Formato do oráculo: estruturado**, não textual. `{"funcao": "soma",
   "args": [2, 3]}` em vez de `{"chamada": "soma(2, 3)"}`. Evita escrever um
   parser de C++ no harness, e é o formato que US-6.5 vai reaproveitar em
   `behavior_traces`. `docs/plans/User Steps.md` é explícito: quem implementar
   primeiro define o formato, e dois formatos concorrentes para a mesma coisa é
   o pior resultado possível.

5. **Golden desde o E01**, regravável por `just examples-bless`, em commit
   separado do commit de comportamento. O golden é ferramenta de revisão, não
   contrato.

6. **Visibilidade na UI não é critério para fechar um degrau.** (Resolve a
   proposta em aberto do §8 da escada, que pedia decisão "antes do E03".) Um
   degrau fecha com os três critérios do §5. A UI entra como PR próprio.

7. **Nada de quarta passada `libclang`.** `function_catalog::
   extract_function_catalog_cancellable` já é a única passada que parseia
   corpos de função — as de `type_catalog.rs` usam
   `CXTranslationUnit_SkipFunctionBodies` de propósito, desde a correção de
   escala do Verovio 5.7.0. A extração de IR é uma extensão daquela travessia.

8. **`Unsupported` vira falha visível, nunca omissão.** Uma função cuja IR
   contenha um nó `Unsupported` é emitida em Dart com corpo que lança
   `UnimplementedError`, mensagem contendo arquivo e linha C++ de origem e o
   motivo, precedida de um comentário `// TODO(syntax-bridge): <motivo>`.
   Assim o pacote continua passando em `dart analyze` (critério 5.2) enquanto o
   problema fica impossível de ignorar.

---

## 5 — Restrições invioláveis

Violar qualquer uma destas invalida a entrega, independentemente dos testes
passarem.

1. **TDD.** Toda mudança comportamental começa com um teste que falha. Regra de
   ouro do `AGENTS.md`.

2. **Nenhum caso especial por exemplo.** É proibido qualquer ramo no extrator
   ou no emissor que dependa de nome de arquivo, de função, ou de id de
   exemplo. Se um degrau só passa com um caso especial, a regra geral ainda não
   foi encontrada e o degrau **não fechou**.

3. **Todos os degraus anteriores continuam verdes.** Um degrau novo que quebra
   um antigo não está pronto, não importa quão mais difícil ele seja.

4. **Silêncio é proibido.** Toda construção C++ que a IR não representa vira um
   nó `Unsupported` com origem (arquivo, linha) e motivo. Omitir código sem
   avisar é a única falha inaceitável, porque produz Dart que compila e está
   errado.

5. **Determinismo desde o primeiro commit.** Ordenação estável em toda coleção
   emitida; nenhuma iteração sobre `HashMap`/`HashSet` cuja ordem alcance a
   saída. Retrofitar determinismo depois é caro; nascer com ele é de graça.
   É o critério 3 de US-8.

6. **Rastreabilidade desde o primeiro commit.** Cada nó da IR carrega sua
   origem C++ (arquivo, linha, coluna) e o emissor a propaga. US-9 (critério 3)
   e US-10 (critério 3) dependem dela; adicioná-la depois significa refazer os
   dois.

7. **Nada escreve fora do diretório do projeto.** Vale para o harness também: o
   oráculo compila C++ e executa binários, e é o mesmo mecanismo que US-6 usará
   sobre input arbitrário do usuário.

8. **Cada degrau fechado atualiza `docs/plans/User Steps.md`** (status parcial
   dos passos tocados, com a fatia coberta escrita explicitamente, não
   "parcial" genérico) **e deixa `NOTES.md` no exemplo**, com o que a armadilha
   ensinou.

---

## 6 — Estrutura do corpus

### 6.1 — Layout

`examples/` na raiz do repositório — fora de `test-resources/`, que guarda
archives binários. Os exemplos são material de leitura humana e precisam ser
texto puro, para que o diff de um PR seja legível.

```
examples/
  E01-funcao-aritmetica/
    example.toml          # metadados do degrau
    input/                # projeto C++ completo e compilável (CMake)
      CMakeLists.txt
      src/aritmetica.hpp
      src/aritmetica.cpp
    expected/             # Dart de referência (golden)
      pubspec.yaml
      lib/aritmetica.dart
    oracle/
      cases.json          # casos de comportamento observável
    decisions.toml        # decisões de US-7 fixadas (ausente até o E03)
    NOTES.md              # o que este degrau ensinou; armadilhas encontradas
```

### 6.2 — `example.toml`

```toml
id = "E01"
nome = "Função aritmética livre"
nivel = 1
status = "esperado-falhar"        # "passa" | "esperado-falhar"
motivo = "emissor Dart ainda não existe"
constroi = ["funcao-livre", "int", "expressao-binaria", "return"]
passos = ["US-7", "US-8", "US-9", "US-10"]
```

### 6.3 — `oracle/cases.json`

```json
{
  "schema_version": 1,
  "casos": [
    { "funcao": "soma", "args": [2, 3], "espera": 5 },
    { "funcao": "soma", "args": [-1, 1], "espera": 0 },
    {
      "funcao": "soma",
      "args": [2147483647, 1],
      "espera": -2147483648,
      "divergencia_conhecida": "overflow-int32"
    }
  ]
}
```

- `espera` é **conferência de sanidade**. A verdade é o C++ executado, não o
  número que o autor do exemplo achou que sairia. Se `espera` divergir da saída
  do C++, o harness falha apontando o exemplo, não o produto.
- `divergencia_conhecida` marca um caso em que C++ e Dart **devem** divergir
  (decisão 3 do §4). O harness exige que a divergência ocorra: se os dois lados
  passarem a concordar, ele falha — mesma disciplina de `status =
  "esperado-falhar"`.

### 6.4 — Formato canônico de valor

O oráculo compara texto. O formato precisa estar definido antes do primeiro
runner, senão os dois lados divergem por formatação e não por comportamento.
Este formato é o embrião de `behavior_traces.entry_json`/`exit_json` de
US-6.5, e adota a política de serialização que US-6.2 já declara:

| Tipo | Forma canônica |
| --- | --- |
| Inteiro | Decimal, com sinal quando negativo |
| Ponto flutuante | **Duas formas**: decimal canônico de ida-e-volta mais os bits crus em hexadecimal. A comparação entre linguagens usa os bits; o humano lê o decimal |
| `bool` | `true` / `false` |
| Agregado (E03) | `{campo: valor, ...}`, campos na **ordem de declaração**, nunca em ordem de hash |

Endereços nunca aparecem. Nenhum timestamp entra no payload.

---

## 7 — Entregas, em ordem

Um PR por item. Cada um tem seu próprio critério de completude; nenhum fecha
sem ele.

### PR 1 — Infra do corpus

**Constrói:** `examples/E01…`, `E02…`, `E03…` (só `input/`, `oracle/` e
`example.toml`); harness em `crates/server/tests/conversion_examples.rs` que
varre o diretório; receita `just examples`.

**Encerra vermelho de propósito.** É a regra de ouro do `AGENTS.md` em escala
de produto: o corpus **é** o teste que falha.

**Completo quando:**
1. `just examples` descobre os três exemplos varrendo `examples/`, sem lista
   fixa no código.
2. Cada exemplo é reportado individualmente como não implementado, com seu
   `motivo` do `example.toml` na saída.
3. Um `example.toml` malformado ou um diretório sem ele produz erro nomeando o
   exemplo, nunca é ignorado em silêncio.
4. O harness falha se um exemplo com `status = "esperado-falhar"` passar.
5. Os três `input/` compilam com `cmake` + `clang++` (provado pelo harness),
   ainda que nada seja transpilado.

### PR 2 — E01, caminho fino

**Constrói:** `crates/server/src/ir/` (`Module`, `Function`, `Param`,
`Type::{Int, Void}`, `Block`, `Return`, `Binary`, `Ref`, `Literal`,
`Unsupported`, todos com origem C++); *lowering* em
`crates/server/src/lower/cpp.rs` como extensão do passe de `function_catalog`;
emissor em `crates/server/src/emit/dart.rs`; orquestração em
`crates/server/src/transpile.rs`; rota `POST /projects/transpile` síncrona em
`server.rs`; receita `just examples-bless`.

**Completo quando:**
1. Transpilar o E01 produz um pacote Dart com `pubspec.yaml` e `lib/`.
2. O Dart gerado é idêntico ao `expected/` do E01 (critério 5.1).
3. `dart analyze` sobre o pacote gerado não reporta erro, e `dart format
   --output=none --set-exit-if-changed` não acusa diferença (critério 5.2 —
   critérios 1 e 2 de US-9).
4. Transpilar duas vezes produz saída idêntica byte a byte (critério 3 de
   US-8).
5. Cada declaração Dart gerada é rastreável até arquivo e linha C++ de origem,
   e isso é afirmado por teste (critério 4 de US-8).
6. Uma construção C++ deliberadamente não suportada no fixture de teste produz
   um nó `Unsupported` com origem e motivo, e o Dart emitido para ela lança
   `UnimplementedError` — sem quebrar `dart analyze` (critério 5 de US-8).
7. Nenhuma nova passada `libclang` foi introduzida (decisão 7 do §4).
8. `just check` e `just lint` limpos.

### PR 3 — Oráculo comportamental

**Constrói:** runner C++ (gera um `main` que chama a função com os argumentos
do caso e imprime em formato canônico; compila com `clang++` usando as flags
reais do `compile_commands.json` do exemplo), runner Dart (`main.dart`
equivalente sobre o Dart **gerado**, executado com `dart run`), comparação
canônica, e o teste de mutação.

**É aqui que o produto prova, pela primeira vez, que converteu de verdade.**

**Completo quando:**
1. Para cada caso do `oracle/cases.json` do E01, a saída canônica do C++
   executado é igual à do Dart executado (critério 5.3).
2. `espera` é conferido contra a saída do C++, e uma divergência culpa o
   exemplo, com mensagem que diz isso.
3. Um caso marcado `divergencia_conhecida` falha o harness se os dois lados
   concordarem.
4. **Teste de mutação:** introduzir uma divergência deliberada no emissor
   (trocar `+` por `-`) faz o oráculo falhar, com origem e com os valores
   esperado e obtido na mensagem. Uma suíte que passa mas não falha quando
   sabotada não está sendo testada. É o critério 3 de US-10.
5. O E01 passa a `status = "passa"` no `example.toml`, e ganha `NOTES.md`.
6. `docs/plans/User Steps.md` registra US-8, US-9 e US-10 como `parcial`, com a
   fatia coberta escrita explicitamente.

### PR 4 — E02, controle de fluxo

**Constrói:** statements na IR (`If`, `While`, `For`, `VarDecl`, `Assign`,
`ExprStmt`), expressões (`Call`, `Unary`), tipos (`Bool`, `Double`), e **o tipo
resolvido em cada nó de expressão**.

**Armadilha, e é o conteúdo real do degrau:** `a / b` entre inteiros trunca em
C++ e produz `double` em Dart — precisa virar `~/`. É o primeiro ponto em que a
tradução deixa de ser textual e passa a exigir os **tipos dos operandos**. Se
a IR do PR 2 não carregar tipo por nó de expressão, é aqui que isso aparece;
por isso a exigência já está no PR 2.

**Completo quando:**
1. Os três critérios do §5 passam para o E02.
2. O `oracle/cases.json` do E02 contém pelo menos um caso de divisão inteira
   com resto (ex.: `7 / 2`) e um de divisão entre `double`, e os dois
   concordam entre C++ e Dart.
3. Contém pelo menos um caso de recursão e um de laço com zero iterações.
4. O E01 continua verde (restrição 3 do §5).
5. `NOTES.md` do E02 registra a armadilha.

### PR 5 — E03, `struct` POD

**Constrói:** agregado na IR (`Record`, `Field`, `Type::Record(usr)`, acesso a
campo, construção de agregado); fatia mínima de US-7 em
`crates/server/src/mapping.rs` (`MappingOption { id, rótulo, descrição,
consequences: Vec<Consequence> }`, `MappingDecision { type_usr, option_id,
decided_at }`); tabela `type_mappings (type_usr TEXT PRIMARY KEY, option_id,
decided_at)` em `project_store.rs`, chaveada pelo `usr` de US-3; leitura de
`decisions.toml` aplicada ao banco antes de transpilar.

**Armadilha, e é a razão de o degrau existir:** `void mover(Ponto p)` **copia**
em C++ e passa **referência** em Dart. Um caso de oráculo que muta `p` dentro
da função e lê fora produz resultados diferentes — silenciosamente, sem erro de
compilação. Este é o primeiro degrau em que o oráculo pega algo que nem o
golden nem o `dart analyze` pegariam.

Há duas abordagens para resolver (cópia no *call site*, ou cópia na entrada da
função para parâmetros agregados por valor). **A escolha é de quem
implementar**, sujeita à restrição 2 do §5 — a regra tem que ser geral, não um
ramo para este exemplo — e precisa ficar registrada em `NOTES.md` com o motivo.

**Completo quando:**
1. Os três critérios do §5 passam para o E03.
2. O `oracle/cases.json` do E03 contém um caso que **muta um parâmetro
   agregado dentro da função e lê o original fora**, e C++ e Dart concordam.
3. `mapping::options_for` devolve **exatamente uma** opção para uma `struct`
   POD sem herança múltipla (critério 1 de US-7), e uma lista **nunca vazia**
   para qualquer outro tipo, com motivo (critério 5 de US-7 e resposta de Q9).
4. Um tipo sem decisão produz falha explícita ou marcação visível no Dart,
   nunca silêncio (critério 5 de US-8).
5. As decisões vêm de `decisions.toml` aplicado ao banco, **sem passar pela
   UI** — condição de testabilidade registrada em US-7.
6. Reabrir o projeto preserva a decisão gravada (critério 4 de US-7).
7. Round-trip de `type_mappings` provado por teste inline em
   `project_store.rs`, no padrão dos demais `replace_*`/`list_*`.
8. E01 e E02 continuam verdes.

### PR 6 — UI

**Constrói:** painel do Dart gerado ao lado do fonte C++, reaproveitando
`client/flutter/lib/src/ui/source_file_viewer.dart`.

**Completo quando:**
1. Abrir um arquivo C++ do projeto e disparar a transpilação mostra o Dart
   correspondente no painel.
2. Teste de widget próprio, com um `ServerClient` falso roteirizado — nunca
   rede, nunca toolchain.
3. Caso ponta a ponta em `client/flutter/test/app_test.dart` (lembrar de
   `tester.ensureVisible` antes de `tap` em painéis dockados).
4. Modelos espelhados à mão em
   `client/flutter/lib/src/project/project_models.dart`, conferidos contra o
   JSON do servidor **no mesmo commit** — esta fronteira já produziu um bug de
   contrato divergente antes.
5. `just check` e `just lint` limpos nos dois lados.

---

## 8 — Critério de completude do corte inteiro

O primeiro corte está pronto quando **todas** as afirmações abaixo forem
verdadeiras e verificáveis por comando:

1. `just examples` roda os três exemplos e todos passam nos três critérios do
   §5 (golden, `dart analyze`, oráculo).
2. Os três `example.toml` estão com `status = "passa"`, e o harness falharia se
   algum estivesse marcado `esperado-falhar` e passasse.
3. O teste de mutação falha quando o emissor é sabotado, e essa falha nomeia
   origem e valores.
4. Transpilar duas vezes produz saída idêntica byte a byte.
5. Não existe, no extrator nem no emissor, nenhum ramo que dependa de nome de
   arquivo, de função ou de id de exemplo.
6. Nenhuma passada `libclang` foi acrescentada às três existentes.
7. `just ci` passa. Se rodado fora do Flatpak, o resumo final diz isso
   explicitamente e nomeia o que ficou pendente de verificação no ambiente de
   destino.
8. Cada exemplo tem `NOTES.md` com a armadilha que ele ensinou.
9. `docs/plans/User Steps.md` registra US-7, US-8, US-9 e US-10 como `parcial`,
   com a fatia coberta descrita — não "parcial" genérico.
10. Nenhum arquivo fora do diretório do projeto foi escrito durante qualquer
    execução do harness.

---

## 9 — O que este corte deliberadamente não responde

Registrado para que ninguém confunda ausência com esquecimento:

- **Viabilidade global de mapeamento (Q9).** O solver de restrições sobre o
  grafo de tipos é o item mais caro de US-7 e é dimensionado pelo E09 (herança
  múltipla). Aqui, `options_for` devolve uma opção só. Mas ele **nasce com a
  forma de solver** — `options_for(declaration, catalog, decisions)`, dependente
  das decisões já tomadas e não só da declaração —, porque trocar validador por
  solver depois muda o contrato e a UI que o consome.
- **Forma do código ponte.** Q9 fechou o *papel* (garantir que a lista de opções
  nunca seja vazia); a forma (adaptador gerado, classe manual com TODO, ou
  `dart:ffi`) segue em aberto e só é forçada pelo E10.
- **Estrutura de pacote multi-arquivo, `part`, dedup de header incluído em N
  TUs.** É a armadilha do E11.
- **Incrementalidade.** Os passes de US-3/US-4/US-5 são refeitos por inteiro a
  cada criação de projeto; a transpilação herda isso. Lacuna conhecida e antiga,
  não introduzida aqui.
- **US-6.** Opcional por decisão de produto. Quando existir, ela **substitui a
  escrita manual** do oráculo por geração automática, sem mudar o formato do
  registro de comportamento. Se mudar o formato, o formato foi mal projetado —
  e é o formato do §6.3/§6.4 que precisa de conserto, não o de US-6.

---

## 10 — Se travar

- **Um degrau só passa com caso especial:** a regra geral ainda não foi
  encontrada. Não fecha o degrau; volte ao desenho da IR.
- **O golden briga com o comportamento:** o golden perde. Regrave-o com
  `just examples-bless`, em commit separado, e leia o diff.
- **`dart analyze` reclama de algo que o oráculo aprova:** o Dart gerado está
  errado mesmo assim. Os dois critérios são conjuntivos.
- **Uma construção C++ do exemplo não cabe na IR:** emita `Unsupported` com
  origem e motivo (decisão 8 do §4) e prossiga. Nunca omita.
- **O oráculo passa mas você não confia:** sabote o emissor e confira que ele
  falha. Uma suíte que não falha quando sabotada não está testando nada.
