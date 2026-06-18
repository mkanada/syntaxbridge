# Estado pronto

Este documento registra exatamente o que ja esta funcionando no repositorio.

## Servidor Rust

- Existe um workspace Cargo na raiz do projeto.
- Existe o crate `syntax-bridge-server` em `crates/server`.
- O crate possui:
  - biblioteca em `crates/server/src/lib.rs`;
  - modulo inicial de fronteira de toolchain em `crates/server/src/toolchain.rs`;
  - binario minimo em `crates/server/src/main.rs`.
- O binario atual inicia corretamente e imprime `syntax-bridge-server`.

## Testes de disponibilidade da toolchain

Existe uma suite de testes de integracao em
`crates/server/tests/toolchain_availability.rs`.

Ela ja valida:

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
- instala o servidor em `/app/libexec/syntax-bridge-server`;
- instala o runner de testes em `/app/bin/syntax-bridge-toolchain-tests`;
- instala o comando principal em `/app/bin/syntax-bridge-server`.

## Scripts de verificacao

Existem dois scripts de verificacao:

- `scripts/test-in-flatpak.sh`
  - executa `cargo --offline test` dentro do SDK Flatpak com Rust e LLVM
    ativados;
- `scripts/test-flatpak-package.sh`
  - constroi o Flatpak;
  - instala o app localmente;
  - executa os testes pelo app instalado com
    `flatpak run --command=syntax-bridge-toolchain-tests dev.syntax_bridge.SyntaxBridge`.

## Validacoes ja executadas com sucesso

Os seguintes comandos ja foram executados com sucesso:

```sh
cargo fmt --check
cargo test --offline
scripts/test-in-flatpak.sh
scripts/test-flatpak-package.sh
flatpak run --command=syntax-bridge-server dev.syntax_bridge.SyntaxBridge
```

Resultado dos testes de toolchain:

- 4 testes passaram;
- 0 testes falharam;
- os testes passaram dentro do Flatpak instalado.

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
- UI Flutter;
- integracao com Dart SDK;
- integracao com KLEE;
- integracao com GoogleTest;
- transpilacao de C/C++ para Dart.

