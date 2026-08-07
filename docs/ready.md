# Estado pronto

Este documento registra exatamente o que ja esta funcionando no repositorio.

## Servidor Rust

- Existe um workspace Cargo na raiz do projeto.
- Existe o crate `syntax-bridge-server` em `crates/server`.
- O crate possui:
  - biblioteca em `crates/server/src/lib.rs`;
  - servidor HTTP minimo em `crates/server/src/server.rs`;
  - modulo inicial de fronteira de toolchain em `crates/server/src/toolchain.rs`;
  - binario em `crates/server/src/main.rs`.
- O binario aceita `--addr HOST:PORT` ou `SYNTAX_BRIDGE_SERVER_ADDR`.
- O endpoint `GET /health` responde JSON com:
  - `service: "syntax-bridge-server"`;
  - `status: "ok"`.

## Cliente Flutter Desktop

- Existe um app Flutter Linux em `client/flutter`.
- A tela inicial mostra o estado de conexao com o servidor Rust.
- A UI le a URL do servidor via `SYNTAX_BRIDGE_SERVER_URL`.
- Quando o endpoint `/health` retorna `status: "ok"`, a tela mostra
  `Connected`.
- Existe teste de widget em `client/flutter/test/app_test.dart` cobrindo o
  estado conectado com um cliente injetado.

## Testes de disponibilidade da toolchain

Existe uma suite de testes de integracao em
`crates/server/tests/toolchain_availability.rs`.

Ela ja valida:

- endpoint HTTP `/health` do servidor Rust;
- parsing de um pequeno arquivo C++ com `libclang`;
- compilacao de um pequeno programa C++ com `clang++`;
- configuracao de um projeto CMake pequeno;
- exportacao e leitura estrutural de `compile_commands.json`;
- geracao de AST C++ com `tree-sitter` e `tree-sitter-cpp`.

O fixture C++ usado pelos testes e pequeno e fica embutido na propria suite.

## Dependencias Rust offline

- As dependencias Rust estao vendorizadas em `vendor/`.
- O Cargo esta configurado em `.cargo/config.toml` para usar as dependencias
  vendorizadas.
- `cargo test --offline` funciona no workspace.

## Flatpak

Existe infraestrutura inicial de Flatpak em `build-aux/flatpak`.

O manifesto atual e:

- `build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`

Ele:

- usa `org.freedesktop.Sdk//25.08`;
- ativa as extensoes:
  - `org.freedesktop.Sdk.Extension.rust-stable`;
  - `org.freedesktop.Sdk.Extension.llvm21`;
- compila o servidor em release;
- compila o binario de testes de toolchain em release;
- instala o bundle Flutter Linux em `/app/lib/syntax-bridge`;
- instala o comando principal em `/app/bin/syntax-bridge`;
- instala o servidor em `/app/libexec/syntax-bridge-server`;
- instala o runner de teste HTTP em `/app/bin/syntax-bridge-server-health-tests`;
- instala o runner de testes em `/app/bin/syntax-bridge-toolchain-tests`;
- instala desktop file, icone e metainfo do app.

O manifesto esta dividido em tres modulos, do mais estavel para o mais volatil,
porque o cache do flatpak-builder e uma cadeia: um miss em um modulo invalida
todos os seguintes.

- `dart-sdk` — baixa e instala o Dart SDK;
- `syntax-bridge-server` — compila o servidor e os binarios de teste;
- `syntax-bridge-app` — instala o bundle Flutter e os wrappers.

As fontes do modulo Rust vem de `rust-src.tar`, um arquivo gerado por
`scripts/test-flatpak-package.sh`. Isso e proposital: o flatpak-builder nao
consegue cachear um modulo que use fonte `type: dir`
(`builder_source_dir_checksum` alimenta o cache com um valor aleatorio, com o
comentario "We can't realistically checksum a directory, so always rebuild"),
mas ele faz checksum do **conteudo** de uma fonte `type: file`. Empacotar as
entradas Rust em um tar deterministico transforma "arvore inalterada" em cache
hit em vez de recompilacao completa.

O comando principal do Flatpak executa `build-aux/flatpak/syntax-bridge`, que:

- sobe o servidor Rust em `127.0.0.1:37651`;
- exporta `SYNTAX_BRIDGE_SERVER_URL`;
- executa o bundle Flutter Linux;
- encerra o servidor quando a UI termina.

## Scripts de verificacao

Existem dois scripts de verificacao:

- `scripts/test-in-flatpak.sh`
  - executa `cargo --offline test` dentro do SDK Flatpak com Rust e LLVM
    ativados;
- `scripts/test-flatpak-package.sh`
  - constroi o bundle Flutter Linux;
  - empacota as entradas Rust em `build-aux/flatpak/rust-src.tar`;
  - constroi o Flatpak;
  - instala o app localmente;
  - executa os testes pelo app instalado com
    `flatpak run --command=syntax-bridge-toolchain-tests dev.syntax_bridge.SyntaxBridge`;
  - executa o teste HTTP pelo app instalado com
    `flatpak run --command=syntax-bridge-server-health-tests dev.syntax_bridge.SyntaxBridge`.

O cache do flatpak-builder fica em `~/.cache/syntax-bridge` (ajustavel por
`FLATPAK_CACHE_ROOT`, `FLATPAK_BUILD_DIR` e `FLATPAK_STATE_DIR`). Antes ele
ficava em `/tmp`, que o systemd-tmpfiles esvazia a cada boot — o que forcava
rebaixar os 222 MB do Dart SDK e recompilar tudo depois de reiniciar a maquina.

## Validacoes ja executadas com sucesso

Os seguintes comandos ja foram executados com sucesso:

```sh
cargo fmt --check
cargo test --offline
flutter test
flutter analyze
flutter build linux --release
scripts/test-in-flatpak.sh
scripts/test-flatpak-package.sh
```

Resultado dos testes de toolchain:

- 4 testes passaram;
- 0 testes falharam;
- os testes passaram dentro do Flatpak instalado.

Resultado do teste HTTP do servidor:

- 1 teste passou;
- 0 testes falharam;
- o teste passou dentro do Flatpak instalado.

## Estado da configuracao Flatpak

A configuracao Flatpak atual e funcional para validar a disponibilidade das
ferramentas dentro do pacote.

Ela ainda nao deve ser considerada a configuracao final ideal do produto, porque
usa `org.freedesktop.Sdk` como runtime para disponibilizar ferramentas de
desenvolvimento no ambiente instalado.

Para o produto final, ainda sera necessario decidir se as ferramentas de runtime
serao:

- empacotadas explicitamente em `/app`;
- fornecidas por extensoes especificas;
- separadas entre um pacote de desenvolvimento/teste e um pacote final mais
  enxuto.

## Ainda nao implementado

Ainda nao ha:

- API de servidor;
- modelo intermediario;
- persistencia SQLite;
- integracao com Dart SDK;
- integracao com KLEE;
- integracao com GoogleTest;
- transpilacao de C/C++ para Dart.
