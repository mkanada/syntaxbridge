# AGENTS.md

Orientacoes para agentes trabalhando neste repositorio.

## Visao do projeto

Syntax Bridge e uma IDE para transpilacao de C/C++ para Dart.

A arquitetura deve permitir expansao futura para outras linguagens de entrada e
saida. Evite acoplar decisoes de dominio diretamente a C/C++ ou Dart quando uma
abstracao simples puder preservar essa extensibilidade sem adicionar complexidade
prematura.

## Arquitetura prevista

- Servidor: Rust.
- Cliente/UI: Flutter.
- Persistencia: SQLite.
- Empacotamento Linux: Flatpak.

Ferramentas previstas para analise dos artefatos de entrada:

- `libclang`
- `clang`
- `clang++`
- `cmake`
- `tree-sitter`

Ferramentas previstas para analise dos artefatos de saida:

- Dart SDK.

Ferramentas previstas para geracao e execucao de testes unitarios de entrada:

- `klee` (fora da v1)
- GoogleTest (`gtest`/`gmock`) — **incorporado**: modulo `googletest` no
  manifesto Flatpak (`build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`),
  construido via CMake a partir do release v1.18.0, instalado em `/app/lib` e
  `/app/include`. Disponibilidade coberta por
  `googletest_compiles_and_runs_a_small_test_suite` em
  `crates/server/tests/toolchain_availability.rs`.

Decisao registrada (Q10 em `docs/plans/User Steps.md`), revista em 2026-08-13:
GoogleTest deixou de estar "fora da v1" e foi incorporado ao manifesto Flatpak,
por decisao explicita do usuario — reversao parcial de Q10. **`klee` continua
fora da v1**: e a peca cara da decisao original (arrastaria LLVM proprio, um
solver SMT e uma biblioteca C substituta), e a fase B de US-6 (geracao de
entradas sinteticas) continua adiada porque depende de `klee` mesmo com
GoogleTest ja disponivel — GoogleTest sozinho materializa/executa casos, mas
nao os descobre. A caracterizacao por execucao real (fase A) segue usando
apenas `cmake`, `clang++` e `llvm-cov`, ja no manifesto antes desta mudanca. A
inclusao de `klee` volta a ser avaliada quando a fase A tiver medido, com
`llvm-cov`, quanta cobertura ela deixa de fora.

## Metodo de desenvolvimento

- Use TDD: toda mudanca comportamental deve comecar com um teste que falha e
  terminar com o teste passando.
- Rode os testes dentro do ambiente Flatpak quando esse ambiente estiver
  disponivel.
- O objetivo de executar os testes no Flatpak e isolar as ferramentas embutidas
  no sistema das ferramentas instaladas na maquina de desenvolvimento.
- Enquanto o ambiente Flatpak ainda nao existir, registre no resumo final quais
  testes foram executados fora dele e qual cobertura ficou pendente.
- **Toda tela ou etapa nova de interacao do usuario no cliente Flutter precisa
  vir acompanhada de um teste de screenshot** (`client/flutter/test/screenshots/`)
  antes de ser considerada concluida — nao so a tela final de um fluxo, mas
  tambem estados intermediarios relevantes (formulario preenchido, progresso,
  erro). Esses testes alimentam a galeria permanente em
  `docs/screenshots/README.md` (gerada por `just screenshots`, mantida
  atualizada por `.github/workflows/screenshots.yml` a cada push para `main`),
  que e como o usuario acompanha o estado visual do produto pelo GitHub sem
  precisar rodar o app localmente.

## Diretrizes de implementacao

- Prefira Rust para componentes de servidor, analise, orquestracao e
  persistencia.
- Prefira Flutter para experiencia de IDE e interface de usuario.
- Mantenha fronteiras claras entre:
  - analise de entrada;
  - geracao de saida;
  - validacao/testes;
  - persistencia;
  - UI.
- Ao adicionar suporte a uma linguagem, trate-a como plugin/adaptador quando
  possivel, em vez de espalhar condicionais por todo o codigo.
- Nao introduza dependencias externas sem justificar a necessidade no contexto da
  arquitetura.
- **A caracterizacao comportamental (US-6) e opcional para o usuario.** O
  produto oferece as ferramentas, mas o usuario pode escolher nao executa-las e
  ainda assim converter o projeto. Nenhum passo posterior (mapeamento, geracao,
  validacao, exportacao) pode te-la como pre-requisito duro: zero dados de
  caracterizacao e um estado normal, nao um estado incompleto. Quando nao houver
  caracterizacao, reporte cobertura de prova zero explicitamente, em vez de
  bloquear o fluxo ou sugerir que a conversao foi verificada.
- **Resolver o mapeamento de tipos entre linguagens e o objetivo principal do
  produto** (Q9 em `docs/plans/User Steps.md`). Apresente ao usuario apenas
  opcoes de mapeamento globalmente viaveis; quando nenhum mapeamento direto for
  viavel, gere codigo ponte que torne a conversao possivel, em vez de declarar o
  tipo nao convertivel.
- **`dynamic` nao e uma solucao de transpilar.** Nunca o introduza para fazer o
  Dart analisar ou para esconder um tipo C++ ainda sem mapeamento. Cada tipo
  precisa de um destino Dart preciso, um adaptador/ponte nomeado, ou uma
  fronteira externa explicitamente modelada; `Type::Unsupported` e um
  diagnostico temporario a eliminar, nao um tipo de saida aceitavel. O mesmo
  vale para bailouts de expressao: eles devem preservar o tipo estatico esperado
  e falhar explicitamente, sem propagar `dynamic`. Ao encontrar um tipo novo,
  registre seu spelling, ocorrencias e direcao de mapeamento antes de escolher a
  implementacao.

## Estado atual

O scaffold existe e o produto ja roda. O roadmap do ponto de vista do usuario
esta em `docs/plans/User Steps.md`, que e a fonte unica: US-1 a US-5 estao
prontos (criacao de projeto e ingestao, lista de arquivos fonte, catalogo de
tipos, usos de tipo, catalogo de funcoes com grafo de chamadas); US-6 em diante
esta planejado.

Estrutura:

- Servidor Rust em `crates/server` (workspace Cargo na raiz), com os modulos de
  analise (`ingest.rs`, `type_catalog.rs`, `source_catalog.rs`,
  `function_catalog.rs`), orquestracao (`jobs.rs`, `progress.rs`,
  `project_service.rs`), rotas (`server.rs`) e persistencia
  (`persistence/project_store.rs`).
- Cliente Flutter em `client/flutter`, com `lib/src/{project,server,ui,io,logging}`.
- Manifesto Flatpak em `build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`,
  com as extensoes `rust-stable`, `llvm21` e os modulos `dart-sdk` e
  `googletest`.

### Comandos

Use as receitas do `justfile`, nao `cargo`/`flutter`/`dart` crus - elas fixam os
diretorios e as flags corretas. `just` sozinho lista todas.

- `just test` - suite preferida, **dentro do Flatpak** (`scripts/test-in-flatpak.sh`).
  E o que o metodo de desenvolvimento acima pede.
- `just test-host` - a mesma suite na maquina de desenvolvimento, quando o
  Flatpak nao estiver disponivel. Registre no resumo final o que rodou fora do
  Flatpak.
- `just check` / `just lint` / `just fmt-check` - verificacao estatica de Rust e
  Flutter.
- `just ci` - passagem completa (`fmt-check` + `lint` + `test`).
- `just run` - verifica, empacota, instala e executa o app Flatpak.
- `just package-build` / `just package-test` - empacotamento Flatpak, sem e com
  os testes de sandbox.
- `just screenshots` - captura as telas do cliente e regenera a galeria
  permanente em `docs/screenshots/README.md`.
- `just screenshots-wip` - publica um Gist com o estado atual da UI (working
  tree, sem exigir commit), para acompanhar trabalho em andamento sem tocar no
  historico do repositorio principal.
