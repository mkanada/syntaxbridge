# Relatório de qualidade de código

Data: 2026-07-04. Escopo: código de produção e testes em `crates/server` e
`client/flutter` (~4.500 linhas, excluindo `vendor/` e `tmp/`).

## Verificações executadas

| Verificação | Resultado |
| --- | --- |
| `cargo clippy --workspace --all-targets` | Limpo |
| `cargo test --workspace` | 11 testes passando |
| `flutter analyze` | Limpo |
| `flutter test` | 9 testes passando |

Observação (conforme AGENTS.md): todas as verificações acima rodaram na máquina
de desenvolvimento, **fora do Flatpak** (`scripts/test-in-flatpak.sh` não foi
usado nesta análise). A cobertura dentro do sandbox permanece pendente de
confirmação para este relatório.

## Pontos fortes

- **Fronteiras de arquitetura respeitadas.** Ingestão (`ingest.rs`), transporte
  HTTP (`server.rs`) e placeholder de toolchain (`toolchain.rs`) estão
  separados; o cliente Flutter fala com o servidor só via `ServerClient`
  (interface), o que permitiu testes de widget com fake sem rede.
- **Testes de integração realistas.** `project_ingest.rs` exercita o fluxo
  completo (tarball/zip → cmake → compile_commands) incluindo um fixture real
  do Verovio e requisições HTTP cruas (inclusive chunked). Os testes Flutter
  cobrem o fluxo de criação de projeto com sucesso e erro.
- **Tratamento de erros no servidor bem estruturado.** `IngestError` distingue
  erro de cliente (400) de erro de servidor (500) via `is_client_error()`, com
  mensagens `Display` úteis.
- **Cuidado com segurança na extração.** Nome de projeto validado contra path
  traversal (`validate_project_name`), listagem do archive validada antes da
  extração, `tar` invocado com `--no-same-owner --no-same-permissions`.
- **Tooling de projeto maduro para a fase atual.** `justfile` com `fmt`,
  `lint`, `test`, `ci`; vendoring do Cargo (`vendor/` + `.cargo/config.toml`)
  coerente com o objetivo de build offline no Flatpak.

## Achados

### Alta prioridade

#### 1. Contrato cliente/servidor divergente: `build_layers` nunca existiu no servidor

O modelo Dart `CreatedProject` (`client/flutter/lib/src/project/project_models.dart:35`)
espera `build_layers` e `build_dependency_layers`, mas o `CreatedProject` do
Rust (`crates/server/src/ingest.rs:19`) não serializa esses campos. Consequências:

- A UI sempre registra "Build layers found: 0" após criar um projeto real
  (`server_status_page.dart:100`), passando informação falsa ao usuário.
- Os testes Flutter validam camadas de build usando dados fabricados pelo fake
  (`test/app_test.dart:55`), cobrindo um comportamento que o produto não tem.
- `build_plan_view.dart` e `build_dependencies_view.dart` são código morto —
  nenhum arquivo os importa (apenas `compilation_units_view.dart` é usado).
- Os defaults silenciosos do `fromJson` (`?? ''`, `?? 'unknown'`, lista vazia)
  mascaram a divergência em vez de acusá-la.

O `#[serde(alias = "project_name")]` em `ingest.rs:13` é outro sintoma de
contrato que evoluiu sem fonte única. Recomendação: ou remover os campos/views
do cliente até o servidor produzi-los (TDD: o teste do servidor viria antes),
ou implementar a resposta no servidor. Em ambos os casos, criar um teste de
contrato que valide o JSON real do servidor contra o parser do cliente.

#### 2. Validação de archive não cobre ataque via symlink

`validate_archive_listing` (`ingest.rs:308`) valida apenas os *nomes* das
entradas. Um tar malicioso pode conter um symlink `dir -> /alvo/qualquer`
seguido da entrada `dir/arquivo`: os nomes passam na validação, mas a extração
escreve fora de `input-source`. A listagem `tar -tzf` nem expõe o alvo do link,
então essa validação não tem como pegar o caso.

O modelo de ameaça atenua (o usuário abre os próprios archives, e rodar `cmake`
sobre eles já é execução de código por design), mas como o servidor aceita
qualquer POST local, vale fechar. Opções: extrair com as crates `tar`/`zip`
(que permitem validar cada entrada, incluindo alvos de link, durante a
extração) em vez de shellar para `tar`/`unzip`; ou rejeitar qualquer entrada de
tipo link na listagem detalhada (`tar -tvzf`).

#### 3. Cliente HTTP sem timeouts

`HttpServerClient` (`http_server_client.dart`) não define timeout de conexão
nem de resposta. Se o servidor travar, `health()` e a UI de status penduram
indefinidamente (o spinner de "Checking server connection" nunca resolve).
`createProject` pode legitimamente demorar (roda cmake), mas `health()` deveria
ter timeout curto. O `HttpClient` interno também nunca é fechado.

#### 4. `server_status_page.dart` concentra 1.000 linhas com responsabilidades misturadas

O arquivo contém o estado da página, o framework de docking
(`_DockedWorkspace`, `_PanelColumn`, `_ConstrainedDockPanel`), e todo o chrome
decorativo (`_TitleBar`, `_ActivityRail`, `_StatusBar`, `_SearchBox`,
`_WorkspaceCodePreview`). É 4x maior que o segundo maior arquivo do cliente e
tende a crescer a cada painel novo. Sugestão de corte natural: chrome do IDE
(title bar, activity rail, status bar) em um arquivo, workspace/docking em
outro, e a página apenas orquestrando estado.

### Média prioridade

#### 5. UI decorativa se apresenta como funcional

- `IdeToolbarIcon` usa `onPressed ?? () {}` (`ide_theme.dart:52`), fazendo
  botões mortos ("Run pipeline", "Settings") parecerem ativos em vez de
  desabilitados (`onPressed: null`).
- Menus "File/Edit/Search/Run/Window", caixa de busca, ícones da activity rail
  e a status bar ("main", "0 errors, 0 warnings", "UTF-8") são estáticos.
- O teste `renders the project workflow inside IDE chrome` afirma
  `find.text('0 errors, 0 warnings')` (`app_test.dart:43`), cimentando o
  placeholder como comportamento testado.

Placeholders visuais são legítimos em protótipo, mas botões devem refletir seu
estado real, e testes não deveriam ancorar em texto decorativo que será
substituído.

#### 6. Falha na criação de projeto deixa diretório órfão e quebra retry

`create_project` cria `project_dir` com `fs::create_dir` (`ingest.rs:178`) e
não remove nada em caso de falha (cmake falhou, archive inválido etc.). O
usuário que corrigir o problema e tentar de novo com o mesmo nome recebe um
`io::Error` cru ("File exists") como 500, sem mensagem amigável. Recomendação:
limpar `project_dir` em falha (ou extrair para diretório temporário e renomear
no sucesso) e introduzir um erro dedicado `ProjectAlreadyExists` (400).

#### 7. Logging ad-hoc e excessivamente verboso no servidor

- `eprintln!` manual com `timestamp_millis()` e helpers duplicados em
  `ingest.rs:481-522` e `server.rs:193-202`.
- Logam-se `PATH` completo, metadados de cada path e cada linha de
  stdout/stderr das ferramentas, sem níveis nem forma de desligar. Isso inclui
  vazar detalhes do ambiente do servidor no stderr em toda requisição.
- O projeto já traz `tracing`/`tracing-core` no grafo de dependências (via
  axum/tokio); adotar `tracing` + `tracing-subscriber` com `RUST_LOG` daria
  níveis e filtragem sem dependência realmente nova. O mesmo vale para o
  `cliLog` do Flutter, que registra toda resposta HTTP integral.

#### 8. Servidor sem limites operacionais

POSTs concorrentes em `/projects` disparam processos `cmake` ilimitados via
`spawn_blocking`. O bind default em `127.0.0.1` mitiga exposição de rede, mas
qualquer processo local pode acionar extração + cmake. Para uma IDE local é
aceitável hoje — vale documentar a decisão e considerar serializar ingestões
(uma por vez) quando houver persistência de estado.

### Baixa prioridade

- **`ingest.rs:36`** — `#[serde(default, ...)]` em `CompilationUnit`, struct
  que só deriva `Serialize`; o `default` é inerte.
- **Dependência de binários externos** (`tar`, `unzip`, `cmake`) sem checagem
  prévia: se ausentes, o erro é um `io::Error` NotFound cru. Uma verificação de
  disponibilidade com mensagem clara melhoraria o diagnóstico fora do Flatpak.
- **`path_picker.dart:30`** — `allowedExtensions: ['zip', 'tgz', 'gz']` aceita
  qualquer `.gz` (não só `.tar.gz`); um `foo.gz` passa no picker e é rejeitado
  pelo servidor como formato não suportado.
- **Constantes mágicas duplicadas na UI** — largura de painel `360` em dois
  pontos (`server_status_page.dart:466` e `:936`), breakpoint `980` em dois
  pontos (`:271` e `:356`). Extrair para constantes nomeadas evita divergência.
- **`main.dart` como fachada de exports** — os `export`s de `src/*` existem
  para os testes; o idiomático é um `lib/syntax_bridge.dart` público, deixando
  `main.dart` só com o entrypoint.
- **`analysis_options.yaml`** usa apenas o `flutter_lints` base; para um
  cliente que faz IO/rede vale habilitar ao menos `unawaited_futures`,
  `discarded_futures` e `prefer_final_locals`.
- **URL default do servidor duplicada** — `DEFAULT_ADDR` em `server.rs:20` e
  `_defaultServerUrl` em `http_server_client.dart:20` precisam ser mantidos em
  sincronia manualmente (37651 nos dois lados hoje; sem teste que garanta).

## Higiene de repositório

- `tmp/` está corretamente ignorado no git, mas contém resíduos volumosos de
  experimentos antigos (`verovio-port2`, `flutter_ide_multi`, `legacybridge`)
  que confundem buscas e análise local — candidatos a remoção.
- `vendor/` com ~3.500 arquivos versionados é coerente com o build offline do
  Flatpak, mas a justificativa não está registrada em lugar nenhum
  (AGENTS.md/README); vale uma linha documentando para que ninguém "limpe" por
  engano.
- Persistência SQLite e o módulo `toolchain.rs` são placeholders declarados —
  consistente com o estado inicial descrito no AGENTS.md, sem ação necessária.

## Recomendações priorizadas

1. Resolver a divergência de contrato `build_layers` (implementar no servidor
   ou remover do cliente) e adicionar teste de contrato ponta a ponta.
2. Trocar a extração via `tar`/`unzip` externos por crates `tar`/`zip` com
   validação de symlink por entrada.
3. Adicionar timeouts ao `HttpServerClient` (curto para `health`, generoso
   para `createProject`).
4. Limpar `project_dir` em falha de ingestão e mapear "projeto já existe" para
   erro 400 dedicado.
5. Adotar `tracing` no servidor com níveis controlados por `RUST_LOG`.
6. Quebrar `server_status_page.dart` (chrome / docking / página) e desabilitar
   de fato os controles decorativos.
