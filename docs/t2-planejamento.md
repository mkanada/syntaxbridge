# Planejamento T2 - Ferramentas Embutidas e Verificacao

## Objetivo

Definir como o aplicativo vai embutir as bibliotecas e ferramentas externas necessarias ao fluxo principal, e criar uma estrutura de testes que comprove que a aplicacao esta usando as ferramentas empacotadas, nao ferramentas instaladas no computador do usuario.

O T2 deve terminar com uma especificacao implementavel para baixar, fixar versoes, empacotar, localizar e validar as ferramentas usadas pelo nucleo Rust e pelo aplicativo Flatpak.

## Escopo

Ferramentas e bibliotecas inicialmente cobertas:

- Tree-sitter e grammar C++.
- CMake.
- Clang, clang++ e libclang.
- clangd.
- Dart SDK.
- Dart analysis server.
- gtest.
- KLEE.
- SQLite.

O foco inicial e Linux desktop via Flatpak. Suporte futuro a outros sistemas pode influenciar a estrutura de diretorios, mas nao deve bloquear o T2.

## Resultado Esperado

Ao final do T2, o projeto deve ter:

- Manifesto das ferramentas embutidas, com nome, versao, origem, checksum e caminho interno esperado.
- Estrutura de diretorios padrao para ferramentas vendorizadas.
- Codigo Rust para resolver caminhos das ferramentas embutidas.
- Comando de diagnostico para validar o ambiente empacotado.
- Testes automatizados que falham quando uma ferramenta do host e usada por engano.
- Plano de integracao com o Flatpak.
- Tela inicial exibindo, a cada execucao, o resultado dos testes das ferramentas ja empacotadas.

## Decisao de Empacotamento

As ferramentas devem ser tratadas como artefatos versionados e reproduziveis. Cada artefato deve ter versao fixa e checksum conhecido.

Estrutura proposta:

```text
vendor/
  linux-x64/
    manifest.toml
    llvm/
      bin/
        clang
        clang++
        clangd
      lib/
        libclang.so
    cmake/
      bin/
        cmake
        ctest
    dart-sdk/
      bin/
        dart
      bin/snapshots/analysis_server.dart.snapshot
    gtest/
      include/
      lib/
    klee/
      bin/
        klee
    licenses/
```

Para SQLite, a decisao preferencial e usar a biblioteca integrada ao crate Rust escolhido, por exemplo `rusqlite` com `bundled`, evitando depender de `sqlite3` do host.

Para Tree-sitter, a decisao preferencial e compilar a grammar C++ junto ao binario Rust por crate, evitando depender de binario externo.

## Manifesto de Ferramentas

Criar um manifesto legivel pela aplicacao, por exemplo `vendor/linux-x64/manifest.toml`.

Campos minimos por ferramenta:

- `id`: identificador interno estavel, como `clang`, `cmake`, `dart`, `klee`.
- `version`: versao esperada.
- `path`: caminho relativo ao diretorio `vendor/linux-x64`.
- `sha256`: checksum do binario principal ou do pacote baixado.
- `probe_args`: argumentos usados para obter versao, como `--version`.
- `expected_output`: trecho esperado na saida do probe.
- `required`: se a ausencia bloqueia o fluxo principal.

Exemplo conceitual:

```toml
[[tools]]
id = "cmake"
version = "3.30.5"
path = "cmake/bin/cmake"
probe_args = ["--version"]
expected_output = "cmake version 3.30.5"
required = true

[[tools]]
id = "clangd"
version = "18.1.8"
path = "llvm/bin/clangd"
probe_args = ["--version"]
expected_output = "clangd version 18.1.8"
required = true
```

## Resolucao de Caminhos

O nucleo Rust nao deve chamar comandos pelo `PATH` do sistema, como `Command::new("cmake")` ou `Command::new("clang")`.

Toda execucao deve passar por um resolvedor central, por exemplo:

- `BundledTools::detect()` localiza o diretorio raiz das ferramentas.
- `BundledTools::tool_path(ToolId::CMake)` retorna um caminho absoluto.
- `ToolRunner::run(ToolId::CMake, args)` executa somente o caminho resolvido.

Regras:

- Em desenvolvimento, a raiz pode vir de uma variavel como `SYNTAX_BRIDGE_VENDOR_DIR`.
- No Flatpak, a raiz deve ser derivada do local instalado da aplicacao, por exemplo abaixo de `/app/`.
- O `PATH` pode ser ajustado para dependencias secundarias, mas o executavel principal deve sempre ser absoluto.
- Variaveis como `LD_LIBRARY_PATH`, `LIBCLANG_PATH` e equivalentes devem apontar primeiro para bibliotecas empacotadas.

## Probes de Verificacao

Cada ferramenta deve ter um probe especifico. O probe deve validar tres coisas:

- O arquivo executado esta dentro da raiz vendorizada esperada.
- A versao retornada corresponde ao manifesto.
- A ferramenta consegue executar uma operacao minima real, nao apenas imprimir versao.

Probes iniciais:

- CMake: `cmake --version` e configuracao de um projeto CMake minimo temporario.
- Clang: `clang++ --version` e compilacao de um `main.cpp` minimo.
- libclang: carregamento via Rust e consulta basica da versao.
- clangd: `clangd --version` e inicializacao LSP minima em teste isolado.
- Dart SDK: `dart --version` e execucao de um arquivo Dart minimo.
- Dart analysis server: inicializacao minima ou verificacao do snapshot dentro do SDK empacotado.
- gtest: compilacao e execucao de um teste C++ minimo usando includes e libs vendorizadas.
- KLEE: `klee --version` e execucao minima em um bitcode simples, se viavel no sandbox.
- SQLite: abrir banco em memoria, criar tabela, inserir e consultar linha.
- Tree-sitter C++: parse de um trecho C++ minimo e verificacao do no raiz esperado.

## Testes Contra Uso Acidental do Host

O ponto central do T2 e provar que uma ferramenta instalada no computador do usuario nao esta sendo usada por engano.

Estrutura de testes proposta:

1. Criar um diretorio temporario `fake-host-bin`.
2. Inserir executaveis falsos chamados `cmake`, `clang`, `clang++`, `clangd`, `dart` e `klee`.
3. Cada executavel falso deve falhar de forma identificavel, por exemplo imprimindo `HOST_TOOL_USED` e retornando codigo diferente de zero.
4. Executar os probes com `PATH` apontando primeiro para `fake-host-bin`.
5. O teste passa somente se nenhum probe executar os binarios falsos.

Esse teste deve ser automatizado no Rust e deve verificar stderr/stdout para garantir que `HOST_TOOL_USED` nunca aparece.

Tambem deve existir um teste negativo controlado para provar que o fake funciona: ao executar `Command::new("cmake")` com o `PATH` contaminado, o binario falso deve ser chamado.

## Fluxo TDD

Implementar o T2 seguindo esta ordem:

1. Criar teste que monta `fake-host-bin` e comprova que uma chamada ingenua por nome cairia no host falso.
2. Criar teste que chama o resolvedor de ferramentas e espera caminho absoluto dentro de `vendor/linux-x64`.
3. Rodar os testes e confirmar falha por falta do resolvedor.
4. Implementar `BundledTools` e `ToolRunner` de forma minima.
5. Criar probes de versao para uma primeira ferramenta, preferencialmente CMake.
6. Rodar os testes ate passarem.
7. Repetir para Clang, Dart, Tree-sitter, SQLite, gtest, clangd e KLEE.
8. Adicionar diagnostico agregado que retorna relatorio estruturado para a UI Flutter.

## Subtarefas de Implementacao Incremental

Cada subtarefa deve terminar com uma versao executavel do sistema. Ao abrir a aplicacao, a tela inicial deve mostrar o resultado do diagnostico das ferramentas ja implementadas, uma por linha.

Formato visual esperado:

```text
Checking CMAKE...ok
Checking SQLite...ok
Checking Tree-sitter C++...ok
```

Quando houver falha, a tela deve mostrar a ferramenta, o status e uma mensagem curta:

```text
Checking CMAKE...failed: bundled binary not found
```

Os itens abaixo devem ser feitos sempre em TDD: primeiro criar ou ajustar o teste que falha, depois implementar a funcionalidade minima, depois executar os testes e validar a aplicacao.

### T2.1 [x] - Infraestrutura de Diagnostico na Tela Inicial

Objetivo: criar a base comum para exibir verificacoes na tela inicial antes de empacotar a primeira ferramenta real, e tambem registrar o mesmo diagnostico na linha de comando durante a execucao da aplicacao.

Entregas:

- Criar o tipo Rust de resultado de diagnostico, com nome da ferramenta, status, caminho usado e mensagem.
- Expor uma funcao via `flutter_rust_bridge` que retorna a lista de verificacoes.
- Atualizar a tela inicial Flutter para renderizar linhas no formato `Checking <TOOL>...<status>`.
- Gerar log na linha de comando com as mesmas verificacoes, no mesmo formato visual usado pela tela inicial.
- Incluir um diagnostico temporario interno, por exemplo `Checking diagnostics pipeline...ok`, apenas para validar a integracao UI/Rust.

Testes:

- Teste Rust para serializacao/retorno do resultado de diagnostico.
- Teste ou verificacao manual da tela inicial mostrando a linha temporaria.
- Verificacao manual executando a aplicacao pelo terminal e confirmando que o log `Checking diagnostics pipeline...ok` aparece na linha de comando.

Criterio de conclusao:

- A aplicacao abre e mostra pelo menos uma linha de diagnostico vinda do Rust.
- A mesma linha de diagnostico aparece no log da linha de comando durante a execucao real.

Status: concluida. Verificado com `cargo test`, `flutter test` e `flutter test integration_test/simple_test.dart -d linux`. A execucao real mostra `Checking diagnostics pipeline...ok` no terminal.

### T2.2 [x] - SQLite Embutido

Objetivo: empacotar SQLite como biblioteca integrada ao nucleo Rust, sem depender do binario `sqlite3` do host.

Entregas:

- Configurar o crate SQLite escolhido para usar biblioteca bundled, preferencialmente `rusqlite` com feature `bundled`.
- Criar probe que abre banco em memoria, cria tabela, insere e consulta uma linha.
- Adicionar o resultado `Checking SQLite...ok` na tela inicial.

Testes:

- Teste unitario do probe SQLite.
- Teste com `PATH` contaminado por fake `sqlite3`, garantindo que o probe nao executa binario externo.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking SQLite...ok`.

Status: concluida. SQLite foi integrado como biblioteca bundled via Rust, com probe em banco em memoria e teste com `PATH` contaminado por `fake-host-bin/sqlite3`. Verificado com `cargo test`, `flutter test` e `flutter test integration_test/simple_test.dart -d linux`. A execucao real mostra `Checking SQLite...ok` no terminal.

### T2.3 [x] - Tree-sitter C++ Embutido

Objetivo: compilar Tree-sitter e a grammar C++ junto ao nucleo Rust.

Entregas:

- Adicionar dependencias Rust de Tree-sitter e grammar C++.
- Criar probe que faz parse de um trecho C++ minimo, por exemplo `int main() { return 0; }`.
- Validar que o no raiz e compativel com uma translation unit C++.
- Adicionar o resultado `Checking Tree-sitter C++...ok` na tela inicial.

Testes:

- Teste unitario do parser C++ minimo.
- Teste garantindo que a verificacao nao depende de binario externo.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Tree-sitter C++...ok`.

Status: concluida. Tree-sitter e grammar C++ foram integrados ao nucleo Rust, com probe de parse para `int main() { return 0; }` e teste com `PATH` contaminado por `fake-host-bin/tree-sitter`. Verificado com `cargo test`, `flutter test` e `flutter test integration_test/simple_test.dart -d linux`. A execucao real mostra `Checking Tree-sitter C++...ok` no terminal.

### T2.4 - CMake Empacotado

Objetivo: empacotar CMake e garantir que o sistema usa o CMake vendorizado, nao o instalado no host.

Entregas:

- Criar ou adaptar mecanismo de download/vendor para CMake Linux x64.
- Registrar CMake no manifesto com versao, caminho e checksum.
- Implementar `BundledTools::tool_path(ToolId::CMake)`.
- Criar probe `cmake --version` usando caminho absoluto.
- Criar probe funcional que configura um projeto CMake minimo em diretorio temporario.
- Adicionar o resultado `Checking CMAKE...ok` na tela inicial.

Testes:

- Teste que cria `fake-host-bin/cmake` e coloca esse diretorio no inicio do `PATH`.
- Teste positivo garantindo que o probe usa o caminho vendorizado absoluto.
- Teste negativo controlado garantindo que uma chamada ingenua `Command::new("cmake")` cairia no fake.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking CMAKE...ok`.

### T2.5 - Clang e Clang++ Empacotados

Objetivo: empacotar compilador C++ e garantir compilacao minima usando somente o binario vendorizado.

Entregas:

- Adicionar Clang e Clang++ ao manifesto.
- Implementar resolucao de caminho para `clang` e `clang++`.
- Criar probe `clang++ --version`.
- Criar probe funcional que compila um `main.cpp` minimo em diretorio temporario.
- Adicionar o resultado `Checking Clang C++...ok` na tela inicial.

Testes:

- Teste com `fake-host-bin/clang` e `fake-host-bin/clang++` no inicio do `PATH`.
- Teste garantindo que a compilacao usa o executavel vendorizado.
- Teste verificando que `HOST_TOOL_USED` nao aparece em stdout/stderr.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Clang C++...ok`.

### T2.6 - libclang Empacotado

Objetivo: empacotar `libclang` para analise semantica e garantir carregamento da biblioteca vendorizada.

Entregas:

- Adicionar `libclang.so` ao manifesto.
- Configurar `LIBCLANG_PATH` ou mecanismo equivalente apontando para a biblioteca empacotada.
- Criar probe que carrega `libclang` e consulta a versao.
- Adicionar o resultado `Checking libclang...ok` na tela inicial.

Testes:

- Teste garantindo que o caminho carregado esta dentro de `vendor/linux-x64/llvm/lib`.
- Teste com variaveis de ambiente contaminadas apontando para local invalido, garantindo que o resolvedor sobrescreve para o caminho vendorizado.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking libclang...ok`.

### T2.7 - clangd Empacotado

Objetivo: empacotar o LSP C++ e validar que ele inicia a partir do binario vendorizado.

Entregas:

- Adicionar `clangd` ao manifesto.
- Implementar resolucao de caminho para `clangd`.
- Criar probe `clangd --version`.
- Criar probe LSP minimo, com inicializacao e encerramento controlados, se viavel no tempo do T2.
- Adicionar o resultado `Checking clangd...ok` na tela inicial.

Testes:

- Teste com `fake-host-bin/clangd` no inicio do `PATH`.
- Teste garantindo que o processo iniciado e o vendorizado.
- Teste verificando timeout para evitar travamento da tela inicial.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking clangd...ok`.

### T2.8 - Dart SDK Empacotado

Objetivo: empacotar Dart SDK para validar codigo Dart gerado sem depender do Dart instalado no host.

Entregas:

- Adicionar Dart SDK ao manifesto.
- Implementar resolucao de caminho para `dart`.
- Criar probe `dart --version`.
- Criar probe funcional que executa um arquivo Dart minimo em diretorio temporario.
- Adicionar o resultado `Checking Dart SDK...ok` na tela inicial.

Testes:

- Teste com `fake-host-bin/dart` no inicio do `PATH`.
- Teste garantindo que o probe usa o Dart vendorizado.
- Teste verificando que um programa Dart minimo retorna codigo zero.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Dart SDK...ok`.

### T2.9 - Dart Analysis Server Empacotado

Objetivo: validar que o analysis server usado pelo futuro LSP Dart vem do SDK empacotado.

Entregas:

- Registrar o snapshot ou comando do analysis server no manifesto.
- Criar probe que verifica a existencia do snapshot dentro do Dart SDK vendorizado.
- Criar probe de inicializacao minima, se viavel sem tornar a abertura da aplicacao lenta.
- Adicionar o resultado `Checking Dart analysis server...ok` na tela inicial.

Testes:

- Teste garantindo que o snapshot esta abaixo do Dart SDK vendorizado.
- Teste com `PATH` contaminado garantindo que nenhum Dart do host e executado.
- Teste com timeout para processo LSP, quando o probe de inicializacao for ativado.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Dart analysis server...ok`.

### T2.10 - gtest Empacotado

Objetivo: disponibilizar gtest para gerar e executar testes C++ do comportamento original.

Entregas:

- Decidir se gtest sera empacotado como fonte, biblioteca precompilada ou modulo CMake interno.
- Registrar includes e libs no manifesto, se aplicavel.
- Criar probe que compila e executa um teste gtest minimo usando o Clang/CMake vendorizado.
- Adicionar o resultado `Checking gtest...ok` na tela inicial.

Testes:

- Teste de compilacao de fixture gtest minima.
- Teste garantindo que CMake e Clang usados pelo probe sao os vendorizados.
- Teste verificando saida de teste gtest com sucesso.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking gtest...ok`.

### T2.11 - KLEE Empacotado

Objetivo: empacotar KLEE ou documentar tecnicamente o bloqueio caso ele nao seja viavel dentro do Flatpak inicial.

Entregas:

- Adicionar KLEE ao manifesto quando houver artefato viavel.
- Implementar resolucao de caminho para `klee`.
- Criar probe `klee --version`.
- Criar probe funcional com bitcode minimo, se viavel no sandbox.
- Adicionar o resultado `Checking KLEE...ok` na tela inicial, ou `Checking KLEE...failed: <motivo>` se houver bloqueio tecnico documentado.

Testes:

- Teste com `fake-host-bin/klee` no inicio do `PATH`.
- Teste garantindo que o probe nao chama KLEE do host.
- Teste funcional com timeout, quando o probe de execucao for ativado.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra o status de KLEE usando somente o resolvedor de ferramentas embutidas.

### T2.12 - Validacao Flatpak Completa

Objetivo: garantir que todas as verificacoes funcionam no aplicativo instalado como Flatpak.

Entregas:

- Incluir `vendor/linux-x64` no pacote Flatpak.
- Garantir permissoes de execucao dos binarios empacotados.
- Garantir caminhos internos estaveis dentro do sandbox.
- Executar a aplicacao instalada e validar a tela inicial com todos os checks.

Testes:

- Teste ou script de verificacao do pacote instalado.
- Execucao manual obrigatoria do Flatpak instalado.
- Registro do resultado final esperado na documentacao de release.

Criterio de conclusao:

- O Flatpak instalado abre e mostra todos os checks implementados na tela inicial, usando somente ferramentas empacotadas.

## Relatorio de Diagnostico

O nucleo Rust deve expor uma funcao para a UI, via `flutter_rust_bridge`, que retorne o estado das ferramentas.

Dados por ferramenta:

- Nome.
- Caminho absoluto usado.
- Versao esperada.
- Versao detectada.
- Status: `ok`, `missing`, `wrong_version`, `execution_failed`, `host_tool_detected`.
- Mensagem tecnica curta.
- Se bloqueia ou nao o fluxo principal.

Esse relatorio deve deixar claro quando a falha e problema do pacote instalado, nao ausencia de dependencia no sistema do usuario.

## Integracao com Flatpak

O Flatpak deve instalar os artefatos em local previsivel dentro do sandbox, preferencialmente abaixo de `/app/syntax-bridge/vendor/linux-x64` ou equivalente.

Pontos a validar:

- Os binarios possuem permissao de execucao.
- Bibliotecas dinamicas empacotadas sao encontradas sem depender do host.
- O sandbox permite criar diretorios temporarios para builds e probes.
- O Dart SDK e clangd conseguem rodar dentro do sandbox.
- KLEE e suas dependencias funcionam ou ficam marcados como risco tecnico documentado.

## Riscos Tecnicos

- KLEE pode exigir dependencias, permissao ou modelo de execucao dificil dentro do Flatpak.
- Misturar versoes diferentes de clang, clangd e libclang pode gerar inconsistencias.
- gtest pode ser melhor distribuido como fonte compilada por projeto de teste, em vez de biblioteca precompilada.
- Algumas ferramentas podem carregar bibliotecas dinamicas do host se `LD_LIBRARY_PATH` nao for controlado.
- Projetos C++ importados podem depender de bibliotecas externas que nao fazem parte do pacote da ferramenta.

## Criterios de Conclusao do T2

O T2 pode ser marcado como concluido quando:

- Existe manifesto de ferramentas com versoes e caminhos esperados.
- Existe resolvedor central de ferramentas embutidas.
- Nenhuma chamada principal a ferramenta externa depende diretamente do `PATH` do host.
- Ha testes automatizados com `fake-host-bin` comprovando que ferramentas do host nao sao usadas.
- Ha probes reais para as ferramentas obrigatorias.
- Ha relatorio de diagnostico consumivel pela interface Flutter.
- O Flatpak instalado executa o diagnostico e mostra que usa os binarios empacotados.

## Proximo Passo Apos T2

Depois do T2, o passo seguinte deve criar um projeto CMake pequeno com codigo C++ real para testar a integracao conjunta das ferramentas. Esse projeto deve validar CMake, Clang, Tree-sitter, gtest e persistencia SQLite em um fluxo unico.
