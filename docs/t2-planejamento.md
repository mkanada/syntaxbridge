# Planejamento T2 - Ferramentas Embutidas e Verificacao

## Objetivo

Definir como o aplicativo vai embutir, via `modules` do Flatpak, as bibliotecas e ferramentas externas necessarias ao fluxo principal, e criar uma estrutura de testes que comprove que a aplicacao esta usando as ferramentas empacotadas, nao ferramentas instaladas no computador do usuario.

O T2 deve terminar com uma especificacao implementavel para baixar, fixar versoes, empacotar como modulos Flatpak, localizar e validar as ferramentas usadas pelo nucleo Rust e pelo aplicativo Flatpak.

Estado atual: T2.1, T2.2 e T2.3 estao concluidos. T2.4, CMake empacotado, e o proximo passo.

## Escopo

Ferramentas e bibliotecas inicialmente cobertas:

- Tree-sitter e grammar C++.
- CMake.
- Clang, clang++ e libclang.
- Dart SDK.
- Dart analysis server.
- gtest.
- KLEE.
- SQLite.

O foco inicial e Linux desktop via Flatpak. Suporte futuro a outros sistemas pode influenciar a estrutura de diretorios, mas nao deve bloquear o T2.

## Resultado Esperado

Ao final do T2, o projeto deve ter:

- Manifesto das ferramentas embutidas, com nome, versao, origem, checksum, modulo Flatpak responsavel e caminho interno esperado.
- Estrutura de diretorios padrao para ferramentas instaladas pelos `modules` do Flatpak.
- Codigo Rust para resolver caminhos das ferramentas embutidas.
- Comando de diagnostico para validar o ambiente empacotado.
- Validacoes automatizadas de que as ferramentas resolvidas estao dentro da raiz empacotada.
- Plano de integracao com o Flatpak.
- Tela inicial exibindo, a cada execucao, o resultado dos testes das ferramentas ja empacotadas.

## Decisao de Empacotamento

As ferramentas devem ser tratadas como artefatos versionados e reproduziveis instalados por `modules` do Flatpak. Cada ferramenta ou biblioteca que o `syntax-bridge` precisa deve ter um modulo Flatpak proprio ou um modulo compartilhado claramente identificado, com versao fixa, origem declarada e checksum conhecido.

Em cada etapa de planejamento e implementacao de uma ferramenta, o fluxo de empacotamento deve seguir esta ordem:

1. Buscar primeiro executaveis ou pacotes ja compilados para Linux x64 publicados oficialmente pelo projeto da ferramenta ou por uma fonte confiavel.
2. Se houver artefato precompilado adequado, criar um modulo Flatpak que baixe esse artefato, valide o checksum e instale apenas os arquivos necessarios abaixo de `/app`.
3. Se nao houver artefato precompilado adequado, incorporar a ferramenta como modulo Flatpak baixando os fontes, validando o checksum e compilando-a durante o build do Flatpak.
4. Registrar no manifesto se o modulo usa binario precompilado ou build a partir dos fontes.
5. Validar que o runtime final nao depende de executaveis ou bibliotecas do host.

Estrutura de instalacao proposta dentro do Flatpak:

```text
/app/syntax-bridge/
  tools/
    manifest.toml
    llvm/
      bin/
        clang
        clang++
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

Para SQLite, a decisao preferencial e usar a biblioteca integrada ao crate Rust escolhido, por exemplo `rusqlite` com `bundled`, compilada dentro do modulo principal da aplicacao no Flatpak, evitando depender de `sqlite3` do host.

Para Tree-sitter, a decisao preferencial e compilar a grammar C++ junto ao binario Rust por crate, dentro do modulo principal da aplicacao no Flatpak, evitando depender de binario externo.

Tree-sitter e necessario para preservar, da melhor forma possivel, comentarios e aspectos visuais do codigo C++ original. Informacoes semanticas nao visuais, como tipos, simbolos, referencias e relacoes chamador-chamado, devem ser obtidas pelo libclang.

Mesmo nos casos integrados ao binario Rust, como SQLite e Tree-sitter, a decisao deve ser documentada como parte do modulo principal do empacotamento Flatpak. Se uma dependencia deixar de ser fornecida por crate bundled no futuro, ela deve seguir a mesma regra: primeiro buscar artefato Linux precompilado; se nao existir, criar modulo Flatpak que baixa e compila os fontes.

## Manifesto de Ferramentas

Criar um manifesto legivel pela aplicacao, por exemplo `/app/syntax-bridge/tools/manifest.toml`.

O manifesto fisico deve ser introduzido quando a primeira ferramenta externa empacotada como modulo Flatpak for adicionada, inicialmente no T2.4. SQLite e Tree-sitter C++ ja estao integrados ao modulo principal Rust e devem aparecer no manifesto como ferramentas/bibliotecas fornecidas pelo modulo principal quando o arquivo for criado.

Campos minimos por ferramenta:

- `id`: identificador interno estavel, como `clang`, `cmake`, `dart`, `klee`.
- `version`: versao esperada.
- `path`: caminho relativo ao diretorio `/app/syntax-bridge/tools` no Flatpak.
- `sha256`: checksum do binario principal ou do pacote baixado.
- `flatpak_module`: nome do modulo Flatpak que instalou a ferramenta.
- `source_kind`: `precompiled_linux_x64` quando usa artefato ja compilado ou `source_build` quando compila a partir dos fontes.
- `source_url`: URL do artefato ou dos fontes usados pelo modulo.
- `probe_args`: argumentos usados para obter versao, como `--version`.
- `expected_output`: trecho esperado na saida do probe.
- `required`: se a ausencia bloqueia o fluxo principal.

Exemplo conceitual:

```toml
[[tools]]
id = "cmake"
version = "3.30.5"
path = "cmake/bin/cmake"
flatpak_module = "cmake"
source_kind = "precompiled_linux_x64"
source_url = "https://example.invalid/cmake-linux-x86_64.tar.gz"
probe_args = ["--version"]
expected_output = "cmake version 3.30.5"
required = true

```

## Resolucao de Caminhos

O nucleo Rust nao deve chamar comandos pelo `PATH` do sistema, como `Command::new("cmake")` ou `Command::new("clang")`.

Toda execucao deve passar por um resolvedor central, por exemplo:

- `BundledTools::detect()` localiza o diretorio raiz das ferramentas.
- `BundledTools::tool_path(ToolId::CMake)` retorna um caminho absoluto.
- `ToolRunner::run(ToolId::CMake, args)` executa somente o caminho resolvido.

Regras:

- Em desenvolvimento, a raiz pode vir de uma variavel como `SYNTAX_BRIDGE_TOOLS_DIR`, apontando para uma arvore equivalente a gerada pelos modulos Flatpak.
- No Flatpak, a raiz deve ser derivada do local instalado da aplicacao, por exemplo `/app/syntax-bridge/tools`.
- O `PATH` pode ser ajustado para dependencias secundarias, mas o executavel principal deve sempre ser absoluto.
- Variaveis como `LD_LIBRARY_PATH`, `LIBCLANG_PATH` e equivalentes devem apontar primeiro para bibliotecas empacotadas.

## Probes de Verificacao

Cada ferramenta deve ter um probe especifico. O probe deve validar tres coisas:

- O arquivo executado esta dentro da raiz instalada pelos modulos Flatpak.
- A versao retornada corresponde ao manifesto.
- A ferramenta consegue executar uma operacao minima real, nao apenas imprimir versao.

Probes iniciais:

- CMake: `cmake --version` e configuracao de um projeto CMake minimo temporario.
- Clang: `clang++ --version` e compilacao de um `main.cpp` minimo.
- libclang: carregamento via Rust, consulta basica da versao e extracao semantica minima de simbolos/tipos.
- Dart SDK: `dart --version` e execucao de um arquivo Dart minimo.
- Dart analysis server: inicializacao minima ou verificacao do snapshot dentro do SDK empacotado.
- gtest: compilacao e execucao de um teste C++ minimo usando includes e libs instaladas pelo modulo Flatpak.
- KLEE: `klee --version` e execucao minima em um bitcode simples, se viavel no sandbox.
- SQLite: abrir banco em memoria, criar tabela, inserir e consultar linha.
- Tree-sitter C++: parse de um trecho C++ minimo e verificacao do no raiz esperado.

## Fluxo TDD

Implementar o T2 seguindo esta ordem:

1. Criar teste que chama o resolvedor de ferramentas e espera caminho absoluto dentro da raiz de ferramentas instalada pelos modulos Flatpak, por exemplo `/app/syntax-bridge/tools` no pacote final.
2. Rodar os testes e confirmar falha por falta do resolvedor.
3. Implementar `BundledTools` e `ToolRunner` de forma minima.
4. Criar probes de versao para uma primeira ferramenta, preferencialmente CMake.
5. Rodar os testes ate passarem.
6. Repetir para LLVM/Clang/libclang, Dart, Tree-sitter, SQLite, gtest e KLEE, sempre registrando se o modulo Flatpak usara artefato Linux precompilado ou build a partir dos fontes.
7. Adicionar diagnostico agregado que retorna relatorio estruturado para a UI Flutter.

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

Resumo das ferramentas:

| Ferramenta | Status | Empacotamento planejado/atual | Proximo passo |
| --- | --- | --- | --- |
| Diagnostics pipeline | Concluido | Modulo principal Rust/Flutter | Manter como base dos novos probes |
| SQLite | Concluido | `rusqlite` com `bundled` no modulo principal Rust | Registrar no manifesto quando ele for criado |
| Tree-sitter C++ | Concluido | Crates Rust no modulo principal Rust | Registrar no manifesto quando ele for criado |
| CMake | Pendente | Modulo Flatpak dedicado | Implementar T2.4 |
| LLVM/Clang/libclang | Pendente | Modulo Flatpak coeso para a mesma distribuicao LLVM | Implementar T2.5 |
| Dart SDK | Pendente | Modulo Flatpak dedicado | Implementar T2.6 |
| Dart analysis server | Pendente | Dentro do Dart SDK empacotado | Implementar T2.7 |
| gtest | Pendente | Modulo Flatpak ou fonte compilada para fixtures | Implementar T2.8 |
| KLEE | Pendente/risco | Modulo Flatpak se viavel; bloqueio tecnico documentado se necessario | Implementar T2.9 |

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

Objetivo: empacotar SQLite como biblioteca integrada ao nucleo Rust no modulo principal do Flatpak, sem depender do binario `sqlite3` do host.

Entregas:

- Configurar o crate SQLite escolhido para usar biblioteca bundled, preferencialmente `rusqlite` com feature `bundled`, compilada pelo modulo principal da aplicacao no Flatpak.
- Documentar no manifesto que SQLite e fornecido pelo modulo principal; se a estrategia bundled deixar de ser viavel, buscar artefato Linux x64 precompilado e, na ausencia dele, criar modulo Flatpak que baixa e compila os fontes.
- Criar probe que abre banco em memoria, cria tabela, insere e consulta uma linha.
- Adicionar o resultado `Checking SQLite...ok` na tela inicial.

Testes:

- Teste unitario do probe SQLite.
- Teste garantindo que o probe nao executa binario externo.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking SQLite...ok`.

Status: concluida. SQLite foi integrado como biblioteca bundled via Rust, compilada pelo modulo principal da aplicacao no Flatpak, com probe em banco em memoria e teste garantindo que nao executa binario externo. Verificado com `cargo test`, `flutter test` e `flutter test integration_test/simple_test.dart -d linux`. A execucao real mostra `Checking SQLite...ok` no terminal.

### T2.3 [x] - Tree-sitter C++ Embutido

Objetivo: compilar Tree-sitter e a grammar C++ junto ao nucleo Rust no modulo principal do Flatpak para preservar comentarios e aspectos visuais do codigo C++ original.

Entregas:

- Adicionar dependencias Rust de Tree-sitter e grammar C++ para compilacao pelo modulo principal da aplicacao no Flatpak.
- Documentar no manifesto que Tree-sitter C++ e fornecido pelo modulo principal; se a estrategia via crate deixar de ser viavel, buscar artefato Linux x64 precompilado e, na ausencia dele, criar modulo Flatpak que baixa e compila os fontes.
- Criar probe que faz parse de um trecho C++ minimo, por exemplo `int main() { return 0; }`.
- Validar que o no raiz e compativel com uma translation unit C++.
- Validar preservacao de comentarios em um trecho C++ minimo, incluindo comentario associado e comentario solto.
- Adicionar o resultado `Checking Tree-sitter C++...ok` na tela inicial.

Testes:

- Teste unitario do parser C++ minimo.
- Teste unitario para captura de comentarios e ranges de origem.
- Teste garantindo que a verificacao nao depende de binario externo.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Tree-sitter C++...ok`.

Status: concluida. Tree-sitter e grammar C++ foram integrados ao nucleo Rust, compilados pelo modulo principal da aplicacao no Flatpak, com probe de parse para `int main() { return 0; }` e teste garantindo que nao depende de binario externo. Verificado com `cargo test`, `flutter test` e `flutter test integration_test/simple_test.dart -d linux`. A execucao real mostra `Checking Tree-sitter C++...ok` no terminal.

### T2.4 - CMake Empacotado

Objetivo: empacotar CMake como modulo Flatpak e garantir que o sistema usa o CMake instalado em `/app`, nao o instalado no host.

Entregas:

- Buscar artefato oficial de CMake ja compilado para Linux x64; se nao houver artefato adequado, criar modulo Flatpak que baixa os fontes e compila CMake.
- Registrar CMake no manifesto com versao, caminho, checksum, nome do modulo Flatpak e tipo de origem.
- Implementar `BundledTools::tool_path(ToolId::CMake)`.
- Criar probe `cmake --version` usando caminho absoluto.
- Criar probe funcional que configura um projeto CMake minimo em diretorio temporario.
- Adicionar o resultado `Checking CMAKE...ok` na tela inicial.

Testes:

- Teste positivo garantindo que o probe usa o caminho absoluto instalado pelo modulo Flatpak.
- Teste garantindo que o caminho resolvido esta dentro da raiz de ferramentas empacotadas.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking CMAKE...ok`.

### T2.5 - LLVM/Clang e libclang Empacotados

Objetivo: empacotar LLVM/Clang como modulo Flatpak unico, garantindo que `clang`, `clang++` e `libclang` venham da mesma distribuicao e versao do LLVM instalada em `/app`.

Entregas:

- Buscar uma unica distribuicao oficial de LLVM/Clang ja compilada para Linux x64 contendo `clang`, `clang++` e `libclang`; se nao houver artefato adequado, criar modulo Flatpak que baixa os fontes e compila LLVM/Clang.
- Registrar no manifesto a versao comum do LLVM usada por `clang`, `clang++` e `libclang`, com modulo Flatpak, tipo de origem, origem unica e checksums dos artefatos relevantes.
- Validar no manifesto que os tres componentes pertencem a mesma distribuicao LLVM e nao podem ser atualizados separadamente.
- Implementar resolucao de caminho para `clang`, `clang++` e `libclang.so`.
- Configurar `LIBCLANG_PATH` ou mecanismo equivalente apontando para a biblioteca empacotada da mesma distribuicao LLVM.
- Criar probe `clang++ --version`.
- Criar probe funcional que compila um `main.cpp` minimo em diretorio temporario.
- Criar probe que carrega `libclang`, consulta a versao e extrai simbolos/tipos de uma translation unit minima.
- Criar teste inicial de relacao chamador-chamado simples via libclang.
- Adicionar os resultados `Checking Clang C++...ok` e `Checking libclang...ok` na tela inicial.

Testes:

- Teste garantindo que a compilacao usa o executavel instalado pelo modulo Flatpak.
- Teste garantindo que os caminhos resolvidos de `clang`, `clang++` e `libclang.so` estao dentro da mesma raiz LLVM empacotada.
- Teste garantindo que a versao detectada de `clang++` e a versao detectada de `libclang` correspondem a mesma versao LLVM registrada no manifesto.
- Teste com variaveis de ambiente apontando para local invalido, garantindo que o resolvedor sobrescreve para o caminho instalado pelo modulo Flatpak.
- Teste garantindo que informacoes semanticas nao visuais venham do libclang, nao de Tree-sitter.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Clang C++...ok` e `Checking libclang...ok`, ambos usando a mesma distribuicao LLVM empacotada.

### T2.6 - Dart SDK Empacotado

Objetivo: empacotar Dart SDK como modulo Flatpak para validar codigo Dart gerado sem depender do Dart instalado no host.

Entregas:

- Buscar artefato oficial do Dart SDK ja compilado para Linux x64; se nao houver artefato adequado, criar modulo Flatpak que baixa os fontes e compila o SDK, ou documentar bloqueio tecnico se o build completo nao for viavel no T2.
- Adicionar Dart SDK ao manifesto com modulo Flatpak, tipo de origem e checksum.
- Implementar resolucao de caminho para `dart`.
- Criar probe `dart --version`.
- Criar probe funcional que executa um arquivo Dart minimo em diretorio temporario.
- Adicionar o resultado `Checking Dart SDK...ok` na tela inicial.

Testes:

- Teste garantindo que o probe usa o Dart instalado pelo modulo Flatpak.
- Teste verificando que um programa Dart minimo retorna codigo zero.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Dart SDK...ok`.

### T2.7 - Dart Analysis Server Empacotado

Objetivo: validar que o Dart analysis server usado para diagnosticos do codigo Dart gerado vem do SDK empacotado pelo modulo Flatpak.

Entregas:

- Buscar o analysis server dentro do artefato oficial do Dart SDK ja compilado para Linux x64; se o SDK precisar ser compilado, garantir que o snapshot seja produzido pelo modulo Flatpak.
- Registrar o snapshot ou comando do analysis server no manifesto com modulo Flatpak, tipo de origem e checksum.
- Criar probe que verifica a existencia do snapshot dentro do Dart SDK instalado pelo modulo Flatpak.
- Criar probe de inicializacao minima, se viavel sem tornar a abertura da aplicacao lenta.
- Adicionar o resultado `Checking Dart analysis server...ok` na tela inicial.

Testes:

- Teste garantindo que o snapshot esta abaixo do Dart SDK instalado pelo modulo Flatpak.
- Teste garantindo que nenhum Dart fora do SDK empacotado e executado.
- Teste com timeout para o processo do analysis server, quando o probe de inicializacao for ativado.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking Dart analysis server...ok`.

### T2.8 - gtest Empacotado

Objetivo: disponibilizar gtest para gerar e executar testes C++ do comportamento original.

Entregas:

- Buscar artefato oficial de gtest ja compilado para Linux x64; se nao houver artefato adequado, criar modulo Flatpak que baixa os fontes e compila gtest.
- Registrar includes e libs no manifesto, com modulo Flatpak, tipo de origem e checksum.
- Criar probe que compila e executa um teste gtest minimo usando o Clang/CMake instalados pelos modulos Flatpak.
- Adicionar o resultado `Checking gtest...ok` na tela inicial.

Testes:

- Teste de compilacao de fixture gtest minima.
- Teste garantindo que CMake e Clang usados pelo probe sao os instalados pelos modulos Flatpak.
- Teste verificando saida de teste gtest com sucesso.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra `Checking gtest...ok`.

### T2.9 - KLEE Empacotado

Objetivo: empacotar KLEE como modulo Flatpak ou documentar tecnicamente o bloqueio caso ele nao seja viavel dentro do Flatpak inicial.

Entregas:

- Buscar artefato oficial de KLEE ja compilado para Linux x64; se nao houver artefato adequado, criar modulo Flatpak que baixa os fontes e compila KLEE, ou documentar bloqueio tecnico se isso nao for viavel no sandbox inicial.
- Adicionar KLEE ao manifesto quando houver artefato viavel, com modulo Flatpak, tipo de origem e checksum.
- Implementar resolucao de caminho para `klee`.
- Criar probe `klee --version`.
- Criar probe funcional com bitcode minimo, se viavel no sandbox.
- Adicionar o resultado `Checking KLEE...ok` na tela inicial, ou `Checking KLEE...failed: <motivo>` se houver bloqueio tecnico documentado.

Testes:

- Teste garantindo que o probe nao chama KLEE do host.
- Teste funcional com timeout, quando o probe de execucao for ativado.

Criterio de conclusao:

- Ao executar a aplicacao, a tela inicial mostra o status de KLEE usando somente o resolvedor de ferramentas embutidas.

### T2.10 - Validacao Flatpak Completa

Objetivo: garantir que todas as verificacoes funcionam no aplicativo instalado como Flatpak.

Entregas:

- Declarar todos os artefatos em `modules` do manifesto Flatpak, um modulo por ferramenta ou por conjunto coeso de ferramentas.
- Garantir que cada modulo busque primeiro artefato Linux x64 precompilado e, quando ele nao existir, baixe os fontes e compile durante o build do Flatpak.
- Garantir permissoes de execucao dos binarios instalados pelos modulos Flatpak.
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

O Flatpak deve instalar os artefatos em local previsivel dentro do sandbox, preferencialmente abaixo de `/app/syntax-bridge/tools` ou equivalente, sempre por meio de `modules` declarados no manifesto Flatpak.

Cada modulo deve documentar a decisao de origem: artefato Linux x64 ja compilado quando disponivel; download dos fontes e compilacao no build do Flatpak quando nao houver artefato adequado.

Pontos a validar:

- Os binarios possuem permissao de execucao.
- Bibliotecas dinamicas empacotadas sao encontradas sem depender do host.
- O sandbox permite criar diretorios temporarios para builds e probes.
- O Dart SDK e o Dart analysis server conseguem rodar dentro do sandbox.
- KLEE e suas dependencias funcionam ou ficam marcados como risco tecnico documentado.

## Riscos Tecnicos

- KLEE pode exigir dependencias, permissao ou modelo de execucao dificil dentro do Flatpak.
- Se `clang`, `clang++` e `libclang` nao vierem da mesma distribuicao LLVM, podem ocorrer inconsistencias de compilacao e analise semantica.
- gtest pode ser melhor distribuido como fonte compilada por projeto de teste, em vez de biblioteca precompilada.
- Algumas ferramentas podem carregar bibliotecas dinamicas do host se `LD_LIBRARY_PATH` nao for controlado.
- Projetos C++ importados podem depender de bibliotecas externas que nao fazem parte do pacote da ferramenta.

## Criterios de Conclusao do T2

O T2 pode ser marcado como concluido quando:

- Existe manifesto de ferramentas com versoes e caminhos esperados.
- Existe resolvedor central de ferramentas embutidas.
- Nenhuma chamada principal a ferramenta externa depende diretamente do `PATH` do host.
- Ha validacoes automatizadas comprovando que as ferramentas resolvidas ficam dentro da raiz empacotada.
- Ha probes reais para as ferramentas obrigatorias.
- Ha relatorio de diagnostico consumivel pela interface Flutter.
- O Flatpak instalado executa o diagnostico e mostra que usa os binarios empacotados.

## Proximo Passo Apos T2

Depois do T2, o passo seguinte deve criar um projeto CMake pequeno com codigo C++ real para testar a integracao conjunta das ferramentas. Esse projeto deve validar CMake, Clang, Tree-sitter, gtest e persistencia SQLite em um fluxo unico.
