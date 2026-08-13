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
- GoogleTest (`gtest`). (fora da v1)

Decisao registrada (Q10 em `docs/plans/User Steps.md`): as duas ficam **fora da
v1** e nao entram no manifesto Flatpak por ora. A geracao de entradas sinteticas
(fase B de US-6) depende delas e esta adiada; a caracterizacao por execucao real
(fase A) usa apenas `cmake`, `clang++` e `llvm-cov`, que ja estao no manifesto.
A inclusao volta a ser avaliada quando a fase A tiver medido, com `llvm-cov`,
quanta cobertura ela deixa de fora.

## Metodo de desenvolvimento

- Use TDD: toda mudanca comportamental deve comecar com um teste que falha e
  terminar com o teste passando.
- Rode os testes dentro do ambiente Flatpak quando esse ambiente estiver
  disponivel.
- O objetivo de executar os testes no Flatpak e isolar as ferramentas embutidas
  no sistema das ferramentas instaladas na maquina de desenvolvimento.
- Enquanto o ambiente Flatpak ainda nao existir, registre no resumo final quais
  testes foram executados fora dele e qual cobertura ficou pendente.

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
  com as extensoes `rust-stable`, `llvm21` e o modulo `dart-sdk`.

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
- `just screenshots` - captura as telas do cliente e gera uma galeria HTML.
