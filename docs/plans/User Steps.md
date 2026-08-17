# Passos de uso do Syntax Bridge

Este documento é o roadmap do produto do ponto de vista do usuário. Cada passo
(`US-N`) descreve o que o usuário consegue fazer ao final dele, o contrato
técnico que o sustenta, as decisões ainda em aberto e as condições sob as quais
o passo pode ser provado por teste.

Regra de ouro (`AGENTS.md`): nenhum passo começa sem um teste que falha. Por
isso todo passo aqui carrega critérios de aceitação redigidos como asserções
verificáveis, e não como intenções.

## Como ler cada passo

- **Status** — `pronto` (existe código e teste), `parcial` (parte existe),
  `planejado` (nada implementado).
- **Depende de** — passos que precisam estar prontos antes.
- **Critérios de aceitação** — asserções testáveis; são a definição de pronto.
- **Condições de testabilidade** — o que precisa existir (fixture, determinismo,
  isolamento, ambiente) para que os critérios acima possam virar teste. Quando
  esta seção não estiver satisfeita, o passo ainda não é implementável por TDD.
- **Roteiro de implementação** — presente nos passos ainda não prontos: a
  sequência concreta, em ordem de execução, que um agente deve seguir para
  implementar o passo por TDD. Cada item nomeia o arquivo que muda e o teste
  que precisa falhar antes dele. Onde o roteiro disser "receita padrão", ver a
  seção "Receita padrão de implementação" logo abaixo do índice — ela descreve
  o caminho que US-3, US-4 e US-5 já percorreram, para não ser reescrito em
  cada passo.

## Índice

| ID | Passo | Status | Depende de |
| --- | --- | --- | --- |
| US-1 | Criação de projeto e ingestão do input | pronto | — |
| US-2 | Lista de arquivos fonte e leitura de conteúdo | pronto | US-1 |
| US-3 | Catálogo de tipos do projeto | pronto | US-2 |
| US-4 | Usos de cada tipo e navegação | pronto | US-3 |
| US-5 | Funções, métodos e macros, e seus usos | pronto | US-3 |
| US-6 | Isolamento e caracterização comportamental (**opcional para o usuário**) | planejado (fase A destravada; fase B fora da v1) | US-5 |
| US-7 | Mapeamento de tipos C++ → Dart | planejado | US-4, US-5 |
| US-8 | Geração do código Dart | planejado | US-7 |
| US-9 | Validação estática do Dart gerado | parcial (granularidade de declaração, não de instrução) | US-8 |
| US-10 | Prova de equivalência comportamental | planejado | US-8 (+ oráculo) |
| US-11 | Exportação do projeto convertido | planejado | US-9, US-10 |
| US-12 | Re-ingestão preservando decisões | planejado | US-7 |

Este arquivo é a fonte única do roadmap. Os antigos `docs/plans/ingest.md` e
`docs/plans/separate-compilation-units.md` foram absorvidos por US-1 e US-6,
respectivamente, e removidos do repositório.

US-6 está desdobrado em cinco sub-passos (US-6.1 a US-6.5). O desdobramento
nasceu de uma decisão arquitetural em aberto, **hoje tomada** (as quatro
respostas da rodada 1, no início do item): US-6 tem duas fases, e a fase A —
execução real instrumentada — não depende de KLEE, de GoogleTest nem de
isolamento, e portanto é implementável no ambiente atual.

`docs/plans/ui-lists.md` é o complemento de interface: enquanto este documento
diz *o que* o usuário consegue fazer em cada passo, aquele diz *onde* cada lista
aparece na UI e o que a interface atual precisa mudar para sustentá-la.

`docs/plans/conversao-guiada-por-exemplos.md` é o complemento de execução: este
documento diz *o que* cada passo entrega, aquele propõe *em que ordem*
construir US-7 a US-10 — atravessando o produto de ponta a ponta com exemplos
C++ → Dart mínimos e engrossando esse caminho um degrau por vez.

---

## Perguntas abertas (rodada 2)

As quatro perguntas da rodada 1 (US-6, OBS 1 e OBS 2) foram respondidas e estão
incorporadas ao corpo do documento — ver "Notas do revisor" em US-6. A revisão
que incorporou aquelas respostas gerou seis novas, Q5 a Q10, e **todas as seis
já foram respondidas**. A decisão de cada uma está escrita por extenso no lugar
onde ela pesa; esta tabela guarda só o veredito e o ponteiro.

Duas delas mudam o produto mais do que o resumo sugere, e vale lê-las por
extenso: **Q9** decidiu, contra a recomendação original, que a viabilidade de
mapeamento é *resolvida* e não apenas alertada — resolver o mapeamento de tipos
entre linguagens é o objetivo principal do produto. E, junto com esta rodada,
ficou decidido que **US-6 inteiro é opcional para o usuário** (ver "US-6 é
opcional de ponta a ponta"), o que proíbe qualquer passo posterior de tê-lo como
pré-requisito duro.

| # | Onde | Decide |
| --- | --- | --- |
| ~~Q5~~ | US-6.2 | **Respondida:** a instrumentação entra por reescrita de uma cópia do fonte (opção a). |
| ~~Q6~~ | US-6.3 | **Respondida:** perfil de execução confirmado, com criação em lote por molde; arquivos de entrada sempre copiados para o projeto. |
| ~~Q7~~ | US-6.3 | **Respondida:** o produto compila o projeto de entrada, só os alvos necessários, dentro do mecanismo de job. |
| ~~Q8~~ | Observações transversais → Modelo intermediário | **Respondida:** a IR deixou de ser fronteira exigida, mas segue existindo como estrutura interna de US-8. |
| ~~Q9~~ | US-7 | **Respondida:** resolver de fato — só opções válidas são apresentadas, e código ponte garante que a lista nunca seja vazia. |
| ~~Q10~~ | US-6.4 / Observações transversais → Ambiente de teste | **Respondida, parcialmente revista em 2026-08-13:** KLEE e GoogleTest adiados originalmente; GoogleTest foi depois incorporado ao manifesto Flatpak (decisão explícita do usuário) — KLEE continua adiado. A via sintética (fase B) continua fora da v1: ainda depende de KLEE. |

---

## Receita padrão de implementação

US-3, US-4 e US-5 foram implementados pela mesma sequência. Ela funcionou, está
provada por testes em todos os níveis, e os roteiros dos passos seguintes
apenas dizem "receita padrão" em vez de repeti-la. Um agente que for implementar
qualquer passo abaixo deve seguir esta ordem, e cada item é um commit
potencialmente independente:

1. **Fixture primeiro, em texto, dentro do próprio teste.** Uma `const &str`
   com o C++ mínimo que exercita o comportamento (padrão de `USAGE_TAXONOMY_CPP`
   em `type_catalog.rs` e `FUNCTIONS_CPP` em `tests/function_catalog.rs`).
   Posições esperadas são **calculadas do texto** (`locate_in`), nunca contadas
   à mão. Um comportamento novo que perturbaria contagens exatas de um fixture
   existente ganha fixture próprio, separado.
2. **Teste de extração que falha.** Em `crates/server/tests/<assunto>.rs`,
   afirmando sobre a estrutura de dados retornada, não sobre a UI.
3. **Modelo de dados + extração**, em `crates/server/src/<assunto>.rs`. Toda
   entidade nova carrega o `usr` do `libclang` como identidade estável (decisão
   de US-3); posição (arquivo/linha/coluna) é dado de navegação, nunca chave.
4. **Persistência**, em `crates/server/src/persistence/project_store.rs`:
   `CREATE TABLE IF NOT EXISTS` em `ProjectStore::open`, um par
   `replace_*`/`list_*`, e teste inline de *round-trip* no mesmo arquivo.
   Coluna nova em tabela que já existe **exige** migração pontual via
   `ensure_column` (padrão de `migrate_type_columns`/`migrate_function_columns`),
   senão projetos criados por versões anteriores quebram ao reabrir.
5. **Serviço**, em `crates/server/src/project_service.rs`: uma struct `*Listing`
   que agrega o que a tela precisa em uma resposta só (padrão de
   `TypeCatalogListing`/`FunctionCatalogListing`), lida do banco, sem reparsear.
6. **Rota**, em `crates/server/src/server.rs`, com struct de `Query` própria, e
   teste em `crates/server/tests/<assunto>_route.rs` que **popula o banco
   diretamente** e nunca carrega `libclang` — é o que mantém a suíte de rotas
   rápida e independente de toolchain.
7. **Modelo do cliente**, em `client/flutter/lib/src/project/project_models.dart`,
   espelhado à mão a partir do JSON do servidor. Esta é a fronteira que já
   produziu um bug de contrato divergente (`build_layers`, ver "Contrato
   cliente/servidor"): ao adicionar um campo, conferir os dois lados no mesmo
   commit.
8. **Cliente**, em `server_client.dart` (abstrato) e `http_server_client.dart`
   (implementação); os testes de widget usam um falso roteirizado, nunca rede.
9. **UI**, em `client/flutter/lib/src/ui/<assunto>_view.dart`, com teste de
   widget próprio e isolado, mais a fiação em `server_status_page.dart`.
10. **Ponta a ponta** em `client/flutter/test/app_test.dart` — lembrando de
    `tester.ensureVisible` antes de `tap` em painéis dockados.
11. **Trabalho longo** (qualquer coisa que passe de segundos): reaproveitar
    `progress::ExtractionProgress` (contadores atômicos), `progress::Cancellation`
    (o `AtomicBool` checado por unidade de trabalho) e o `JobRegistry` de
    `jobs.rs`, acrescentando uma variante a `JobPhase` e um campo a
    `CreationProgress`. Nunca inventar um segundo mecanismo de job.

Duas regras de disciplina, importadas de `conversao-guiada-por-exemplos.md` §8
porque valem para todos os passos, não só para a escada de exemplos:

- **Silêncio é proibido.** Toda construção que o produto não sabe tratar vira um
  registro explícito com origem (arquivo, linha) e motivo — nunca uma omissão.
  É a regra que já produziu `CallResolution::Unresolved` em US-5, em vez de
  simplesmente descartar chamadas indiretas.
- **Nenhum caso especial por fixture.** Um ramo no código que dependa de nome de
  arquivo, de função ou de projeto significa que a regra geral ainda não foi
  encontrada.

Rodar testes: `just test` (dentro do Flatpak) ou `just test-host`. Sempre pelas
receitas do `justfile`, não por `cargo`/`flutter` cru.

---

## US-1 — Criação de projeto e ingestão do input

**Status:** pronto · **Depende de:** — ·
**Implementação:** `crates/server/src/ingest.rs`,
`crates/server/src/project_service.rs`, `crates/server/src/jobs.rs`,
`crates/server/src/progress.rs`,
`client/flutter/lib/src/ui/new_project_page.dart`,
`client/flutter/lib/src/ui/creating_project_page.dart` ·
**Testes:** `crates/server/tests/project_ingest.rs`,
`crates/server/tests/verovio_5_7_0_import_diagnosis.rs`,
`client/flutter/test/creating_project_page_test.dart`,
`client/flutter/test/app_test.dart`

### Objetivo do usuário

Partir de um arquivo compactado com código C/C++ e chegar a um projeto do
Syntax Bridge aberto, com a lista de unidades de compilação visível. Assim que
os parâmetros do formulário são válidos, a tela muda imediatamente para uma
tela de progresso com log e barra de progresso — a criação em si roda em
segundo plano no servidor e pode levar minutos num projeto real (ver a
observação "Custo" abaixo).

### Fluxo

- Especificar nome do projeto e diretório de trabalho.
- Escolher um arquivo `.tar.gz` ou `.zip`, que é descompactado no diretório do
  projeto, dentro do subdiretório `input-source`.
- O sistema identifica os arquivos do projeto CMake, roda-o com
  `CMAKE_EXPORT_COMPILE_COMMANDS` habilitado e obtém a lista de *compilation
  units* a partir de `compile_commands.json`.
- O cliente muda de tela assim que os três campos são válidos (não espera o
  servidor) e passa a consultar o progresso periodicamente até a criação
  terminar, com sucesso ou falha.

### Contrato de API

- `POST /projects` → `202 Accepted` com `{"job_id": "…"}`. Não bloqueia mais na
  ingestão nem na extração `libclang` (ver "Custo" abaixo) — inicia um job em
  segundo plano e devolve na hora.
- `GET /projects/jobs/{job_id}` → o estado do job:
  - em andamento: `{"status":"running","phase":"ingesting"|"cataloging_types"|
    "discovering_source_files"|"persisting","type_catalog":{"completed":N,
    "total":M},"source_catalog":{"completed":N,"total":M}}` (a fase é derivada
    dos contadores, não guardada à parte);
  - sucesso: `{"status":"succeeded","project": CreatedProject}`;
  - falha: `{"status":"failed","message":"…","is_client_error":bool}`;
  - id desconhecido: `404`.
- `GET /projects` → últimos 5 projetos (`ProjectRecord`)
- `POST /projects/open` → `LoadedProject` (recarrega sem re-ingerir)

### Persistência

`projects` no banco global; `compilation_units` no `project.db` de cada
projeto. Os jobs de criação **não** são persistidos — vivem só em memória no
processo do servidor (`crates/server/src/jobs.rs`), então reiniciar o servidor
com uma criação em andamento perde o job (o cliente veria `404` ao consultar).

### Observações e decisões em aberto

- **Resolvido: custo da extração `libclang` deixou de bloquear a requisição.**
  Um relato real de "trava no meio da importação" com o Verovio 5.7.0 (não o
  fixture de 6.2.0 já versionado; reproduzido em
  `crates/server/tests/verovio_5_7_0_import_diagnosis.rs`, que não roda por
  padrão) revelou que `project_service::create_project` fazia duas passadas
  completas de `libclang` sobre as 291 unidades de compilação — uma em
  `type_catalog`, outra, independente, em `source_catalog` — cada uma levando
  minutos num único núcleo, dentro de uma única requisição HTTP síncrona, sem
  nenhum log de progresso. A resposta em duas partes: (1) as duas passadas
  agora paralelizam entre os núcleos disponíveis (um `CXIndex` por thread de
  trabalho — a forma documentada de paralelizar `libclang` com segurança,
  já que compartilhar um índice entre threads não é seguro), cortando o tempo
  total de ~11min para ~3min neste ambiente de 4 núcleos; (2) `POST /projects`
  agora inicia um job em segundo plano e devolve na hora, com
  `GET /projects/jobs/{id}` reportando progresso real (via os mesmos contadores
  atômicos que as passadas paralelas já atualizam) para o cliente exibir. Isso
  é a primeira instância concreta do item transversal "Trabalho longo:
  progresso, cancelamento, incrementalidade" — os demais passos (US-4, US-6)
  ainda precisam do próprio mecanismo de job, este é só o primeiro caso.
- **Só CMake.** Projetos com Makefile, autotools ou *header-only* não têm
  caminho. Decidir se o produto exige CMake na v1 e falha explicitamente, ou se
  aceita um `compile_commands.json` fornecido pelo próprio usuário como
  alternativa de escape.
- **Falha de `configure` por dependência ausente** é o cenário mais provável em
  produção: o Flatpak não tem rede nem gerenciador de pacotes, então qualquer
  `find_package` de biblioteca externa quebra. Falta definir a mensagem, o
  diagnóstico apresentado e se o projeto fica gravado em estado incompleto.
- **Multiconfiguração.** Nada define qual *target*, *preset* ou modo
  (Debug/Release) é usado. Isso muda o conteúdo de `compile_commands.json` e,
  por consequência, tudo que vem depois.
- **`compile_commands.json` ausente ou vazio** após um `configure` bem-sucedido
  não é tratado como caso distinto de falha de configure.
- **Limites de entrada** (tamanho do archive, número de arquivos, profundidade)
  não estão definidos. A validação atual cobre segurança (path traversal,
  entradas inseguras), não escala.
- **Reabertura já existe e é mais barata que a re-ingestão**, mas
  `LoadedProject` devolve menos dados que `CreatedProject` — ver US-3.
- **Jobs de criação nunca são removidos do registro em memória.** Aceitável por
  ora (mesma decisão já tomada para o registro de jobs em geral); revisitar se
  um servidor de longa duração acumulando um job por projeto criado alguma vez
  importar na prática.

### Critérios de aceitação (testáveis)

1. Dado um `.tar.gz` e um `.zip` com o mesmo projeto CMake, ambos produzem a
   mesma lista de unidades de compilação.
2. O conteúdo é extraído sob `<projeto>/input-source` e nenhuma entrada do
   archive escapa desse diretório.
3. Um nome de projeto com `..` ou separador de caminho é rejeitado com erro de
   cliente (`is_client_error: true` no job), não de servidor.
4. Após a criação, `compilation_units` no `project.db` contém exatamente as
   entradas devolvidas na resposta.
5. Reabrir o projeto devolve as mesmas unidades de compilação sem executar
   CMake novamente.
6. Um diretório sem `project.db` devolve 404, não 500.
7. Consultar um `job_id` desconhecido devolve 404.
8. Enquanto um job está em andamento, `GET /projects/jobs/{id}` reflete
   progresso real (contadores crescentes), não um valor estático.
9. Na UI, submeter o formulário com parâmetros válidos troca de tela
   imediatamente, antes de qualquer resposta do servidor.

### Condições de testabilidade

- Fixture pequeno e versionado: `test-resources/sample-cmake-project.tar.gz`.
- Um fixture grande e real (Verovio) para provar escala — já exercitado, tanto
  a 6.2.0 (`crates/server/tests/fixtures/verovio/`, usada por padrão) quanto a
  5.7.0 real do usuário (`test-resources/verovio-version-5.7.0.tar.gz`, só no
  teste `#[ignore]`d de diagnóstico).
- Os testes precisam de `cmake` e `clang++` no PATH; dentro do Flatpak isso vem
  das extensões do SDK. Fora dele, o resultado depende da máquina e deve ser
  registrado como tal.
- Diretórios de trabalho temporários e descartáveis por teste: nenhum teste
  pode depender de estado deixado por outro.
- Testes de widget que dependem do polling usam um `ServerClient` falso
  roteirizado (uma lista de respostas, uma por chamada) em vez de temporizador
  real, para não depender de tempo de parede.

---

## US-2 — Lista de arquivos fonte e leitura de conteúdo

**Status:** pronto · **Depende de:** US-1 ·
**Implementação:** `crates/server/src/source_catalog.rs`,
`client/flutter/lib/src/ui/source_files_view.dart`,
`client/flutter/lib/src/ui/source_file_viewer.dart` ·
**Testes:** `crates/server/tests/source_files.rs`, `client/flutter/test/app_test.dart`

### Objetivo do usuário

Ver a lista de todos os arquivos fonte do projeto e, ao clicar em um deles, ler
seu código.

### Contrato de API

- `GET /projects/source-file?path=…` → conteúdo do arquivo
- A lista chega junto de `CreatedProject.source_files` e
  `LoadedProject.source_files` (`SourceFile { path, kind }`, com `kind` em
  `translation_unit` | `header`).

### Persistência

Tabela `source_files` no `project.db`.

### Observações e decisões em aberto

- **"Todos os arquivos fonte" é ambíguo.** `compile_commands.json` lista apenas
  unidades de tradução; os headers são descobertos por inclusão
  (`clang_getInclusions`). A consequência é que *só existe no catálogo o que
  alguma TU alcança*.
- **Código não alcançado fica invisível:** arquivos atrás de `#ifdef` desligado,
  código morto, ou fontes de um target que não foi configurado. Para uma
  ferramenta de transpilação isso é material — o usuário precisa saber que
  aquele arquivo existe no disco e não entrou na análise.
- **Terceiros vendorizados dentro do próprio tarball** entram no catálogo como
  se fossem código do projeto, porque o critério atual é geográfico
  (`starts_with(project_root)`). Falta um conceito explícito de *código do
  projeto* vs. *dependência embutida*, com regra configurável. Essa decisão
  contamina US-3, US-4, US-5 e o volume de trabalho de conversão.
- **Arquivos gerados pelo build** (headers de configuração, saídas de
  `configure_file`) aparecem misturados aos escritos à mão e provavelmente
  merecem `kind` próprio.
- Falta decidir se o conteúdo exibido é o do disco ou o pós-preprocessamento —
  são coisas diferentes e o usuário vai comparar com o que US-4 marca.

### Critérios de aceitação (testáveis)

1. O catálogo contém cada unidade de tradução e cada header local do projeto
   que ela inclui, deduplicados e ordenados por caminho.
2. Headers de sistema não aparecem no catálogo.
3. `GET /projects/source-file` devolve o conteúdo exato do arquivo em disco.
4. Um `path` que aponte para fora do diretório do projeto é rejeitado, mesmo
   quando codificado ou usando `..`.
5. Na UI, clicar em um item da lista exibe o conteúdo correspondente.

### Condições de testabilidade

- O fixture precisa ter diretórios aninhados e pelo menos um header incluído
  transitivamente (header que inclui outro header), senão o teste não distingue
  descoberta transitiva de descoberta direta.
- O cliente Flutter precisa falar com um `ServerClient` falso, para que os
  testes de widget não dependam de rede nem de toolchain — já é o caso.
- Ordenação estável do catálogo, senão os testes ficam intermitentes.

---

## US-3 — Catálogo de tipos do projeto

**Status:** pronto · **Depende de:** US-2 ·
**Implementação:** `crates/server/src/type_catalog.rs`,
`crates/server/src/persistence/project_store.rs`,
`crates/server/src/project_service.rs` (`list_types`),
`crates/server/src/server.rs` (`GET /projects/types`),
`client/flutter/lib/src/ui/types_view.dart`,
`client/flutter/lib/src/ui/source_file_viewer.dart`,
`client/flutter/lib/src/ui/server_status_page.dart` ·
**Testes:** `crates/server/tests/type_catalog.rs`,
`crates/server/tests/type_catalog_route.rs`,
`crates/server/src/persistence/project_store.rs` (testes inline),
`client/flutter/test/types_view_test.dart`,
`client/flutter/test/source_file_viewer_test.dart`,
`client/flutter/test/app_test.dart`

### Objetivo do usuário

Ver, em forma de tabela, todos os tipos definidos no projeto, com nome,
namespace e espécie (struct, class, union, enum, typedef, type alias, macro).
Tipos primitivos e tipos de headers padrão fora do projeto são ignorados.
Clicar em um tipo abre o arquivo onde ele é definido, com seu corpo
destacado.

### Contrato de API

- `POST /projects` e `POST /projects/open` continuam sem devolver o catálogo:
  `CreatedProject`/`LoadedProject` não carregam `type_catalog`.
- `GET /projects/types?project_dir=…` → `{ "types": [TypeDeclaration] }`, lido
  direto do `project.db`, sem reparsear. Resolve a lacuna que existia entre
  criação e reabertura (ver decisão abaixo).

### Persistência

Tabelas `type_declarations` e `type_dependencies` no `project.db`.

### Observações e decisões em aberto

- **Resolvido:** a rota `GET /projects/types` foi adicionada — a opção
  recomendada de escalar melhor para US-4, em vez de inchar `LoadedProject`.
  O painel navegador "Types" já existe no cliente, ao lado do Explorer (mesmo
  lado esquerdo, com abas — ver `docs/plans/ui-lists.md`).
- **Resolvido:** `TypeDeclaration` ganhou `namespace` (cadeia de namespaces
  envolventes, unida por `::`, extraída via `clang_getCursorSemanticParent`) e
  `end_line`/`end_column` (extensão da declaração, via `clang_getCursorExtent`),
  persistidos em `type_declarations`/`type_dependencies`. A tabela de tipos
  agora mostra o nome qualificado (`geometry::Shape`), o que resolve a
  ambiguidade visual entre homônimos — critério 3 abaixo. Clicar em um tipo
  abre seu arquivo de origem, rola até a declaração e destaca o corpo inteiro
  (`SourceFileViewer` ganhou `highlightStartLine`/`highlightEndLine`), usando
  `IdePalette.selection`, até então declarada e não utilizada.
- **Resolvido: identidade estável de tipo via USR do libclang.**
  `TypeDeclaration` ganhou `usr` (`clang_getCursorUSR` em `describe_cursor`,
  `type_catalog.rs`), persistido como coluna `usr` em `type_declarations` e
  `caller_usr`/`callee_usr` em `type_dependencies` (com migração em
  `migrate_type_columns`, mesmo padrão da migração de `namespace`/extensão). A
  deduplicação entre unidades de compilação — antes posicional
  (`kind, name, file, line, column`) — agora usa o USR como chave
  (`declaration_identity` em `type_catalog.rs`), com o fallback posicional
  reservado ao caso (raro) de um `libclang` que não forneça USR para um dado
  cursor. Provado por
  `type_declaration_usr_is_stable_across_incidental_line_shifts` (o USR de um
  tipo não muda quando um edit insere linhas em branco antes dele, embora sua
  `line` mude) e por
  `type_declaration_usr_distinguishes_namespaced_homonyms_with_libclang` (dois
  `Point` em namespaces diferentes têm USRs distintos), ambos em
  `crates/server/tests/type_catalog.rs`. Isso fecha o item que mantinha este
  passo como `parcial`: US-4 em diante já podem referenciar tipos pelo USR em
  vez de posição.
- **O texto original mistura tipos e funções.** Funções e métodos pertencem a
  US-5; manter os dois na mesma lista confunde a modelagem e a UI.
- **Faltam decisões sobre:** templates e suas instanciações/especializações,
  namespaces anônimos (hoje silenciosamente omitidos do nome qualificado, em
  vez de representados) e *inline* (hoje tratados como namespace comum,
  aparecendo no nome qualificado quando não deveriam), *forward declaration*
  vs. definição, tipos aninhados e membros, enums com escopo.
- **Resolvido (parcial):** macros não têm tipo nem escopo, e boa parte não é
  conversível — `TypeDeclarationKind` ganhou `ConstantMacro`, `FunctionMacro`,
  `HeaderGuard` e `AnnotationMacro` no lugar do antigo `Macro` genérico
  (`classify_macro` em `type_catalog.rs`, via `clang_Cursor_isMacroFunctionLike`/
  `isMacroBuiltin` e tokenização do corpo do macro). `HeaderGuard` (o `#define`
  de uma guarda `#ifndef`/`#define`, detectado por heurística de nome+posição)
  e `AnnotationMacro` (outras macros sem valor, ex. `#define MYLIB_API`) são
  filtrados na UI (`TypesView.isUserVisible`) por não terem nada a mostrar ao
  usuário; `ConstantMacro`/`FunctionMacro` aparecem numa seção "Constants &
  macros" à parte dos tipos nomeados. Builtins do compilador (`__STDC__` etc.)
  são descartados no catálogo. **Ainda falta:** macro de compilação
  condicional (`#ifdef`/`#if` fora de guardas de header) não tem representação
  própria — hoje cai em `AnnotationMacro`. Decisão: não abordado agora, ver
  "Preprocessing record de macros" nas observações transversais. O destino de
  cada subtipo em Dart (US-7) ainda não foi definido.
- O grafo de dependências (`TypeDependency`) já implementado não estava
  previsto no plano original e é mais valioso do que a lista: é ele que dá
  ordem topológica de geração em US-8 e fecho transitivo em US-6.

### Critérios de aceitação (testáveis)

1. Para o fixture, o catálogo contém exatamente os tipos declarados no projeto,
   com espécie correta para cada um.
2. Nenhum tipo declarado em header de sistema aparece no catálogo.
3. Tipos com o mesmo nome em namespaces diferentes aparecem como entradas
   distintas e distinguíveis. **Satisfeito** por
   `create_project_catalogs_namespace_and_extent_with_libclang` (fixture
   próprio, não o fixture combinado abaixo) e pelo nome qualificado exibido em
   `TypesView`.
4. Uma TU que o libclang não consegue parsear é ignorada sem derrubar a
   extração das demais.
5. O grafo de dependências contém uma aresta para cada campo, classe base e
   tipo subjacente de typedef/alias, sem duplicatas e sem autorreferência.
6. Reabrir um projeto devolve o mesmo catálogo gravado, sem reparsear.
7. Na UI, a tabela exibe nome (qualificado por namespace) e espécie de cada
   tipo; clicar em uma linha abre o arquivo de origem com o corpo do tipo
   destacado.

### Condições de testabilidade

- O fixture precisa conter, deliberadamente, ao menos: um struct, uma classe
  com herança, uma union, um enum, um typedef, um `using` alias, uma macro, um
  namespace, e dois tipos homônimos em namespaces distintos. Sem isso o
  critério 1 não é exercitável em um único fixture representativo — o critério
  3 já tem cobertura própria (ver acima) com um fixture menor e dedicado.
- `libclang` precisa estar carregável no ambiente de teste; o teste deve falhar
  com mensagem clara quando não estiver, em vez de passar vazio.
- Ordenação determinística do catálogo antes de qualquer comparação.

---

## US-4 — Usos de cada tipo e navegação

**Status:** pronto — extração, persistência, rotas e navegação na UI existem
para a taxonomia de nível de assinatura (ver decisão de taxonomia abaixo, que
deixa usos de nível de expressão — *cast*, `sizeof`, `new`, argumento de
template — deliberadamente fora de escopo em vez de pendentes); cancelamento
de indexação em projetos grandes (critério 7) está implementado e testado ·
**Depende de:** US-3 ·
**Implementação:** `crates/server/src/type_catalog.rs` (`TypeUsageKind`,
`TypeUsage`, `push_usage`, extensão de `visit_cursor`/
`record_member_dependency`, `extract_type_catalog_cancellable`),
`crates/server/src/source_catalog.rs` (`extract_source_files_cancellable`),
`crates/server/src/progress.rs` (`Cancellation`),
`crates/server/src/persistence/project_store.rs`
(tabela `type_usages`, `replace_type_usages`, `list_type_usages_for`,
`type_usage_counts`), `crates/server/src/project_service.rs`
(`TypeCatalogListing`, `list_type_usages`, `CreationProgress::cancellation`,
`ProjectCreationError::is_cancelled`), `crates/server/src/jobs.rs`
(`ProjectCreationJob::cancel`/`cancel_requested`), `crates/server/src/server.rs`
(`GET /projects/types/usages`, `usage_counts` em `GET /projects/types`,
`DELETE /projects/jobs/{job_id}`, estados `cancelling`/`cancelled` em
`GET /projects/jobs/{job_id}`),
`client/flutter/lib/src/project/project_models.dart` (`TypeUsage`,
`TypeUsageKind`, `TypeCatalogListing`, estados `cancelling`/`cancelled` de
`ProjectCreationJobStatus`),
`client/flutter/lib/src/server/server_client.dart`/`http_server_client.dart`
(`cancelCreateProject`),
`client/flutter/lib/src/ui/types_view.dart` (coluna e ordenação por número de
usos), `client/flutter/lib/src/ui/usages_view.dart` (painel de usos),
`client/flutter/lib/src/ui/server_status_page.dart` (fiação entre os dois),
`client/flutter/lib/src/ui/creating_project_page.dart` (botão "Cancel") ·
**Testes:** `crates/server/tests/type_catalog.rs`
(`extract_type_catalog_records_usages_across_the_defined_taxonomy`,
`extract_type_catalog_stops_early_when_cancelled`),
`crates/server/tests/source_files.rs`
(`extract_source_files_stops_early_when_cancelled`),
`crates/server/tests/type_catalog_route.rs`, testes inline em
`persistence/project_store.rs`, `jobs.rs`, `progress.rs`,
`crates/server/tests/project_ingest.rs`
(`create_project_reports_cancellation_and_persists_nothing`,
`cancel_job_endpoint_returns_not_found_for_an_unknown_job`,
`cancel_job_endpoint_does_not_alter_an_already_finished_job`,
`create_project_stops_within_seconds_of_cancellation_on_a_real_project`,
`#[ignore]`d — ver "Condições de testabilidade"),
`client/flutter/test/types_view_test.dart`,
`client/flutter/test/usages_view_test.dart`,
`client/flutter/test/creating_project_page_test.dart`,
`client/flutter/test/app_test.dart`

### Objetivo do usuário

Navegar entre tipos, do código fonte para a lista e da lista para o código.
Clicar em um item da lista mostra imediatamente todos os locais de uso. A lista
apresenta o número de usos e permite ordenar, crescente e decrescente, por nome
e por número de usos.

O *scan* é feito antes, de modo que a navegação seja imediata.

### Observações e decisões em aberto

- **Resolvido (parcial): taxonomia de "uso".** Em vez da lista completa
  originalmente cogitada (declaração de variável, instanciação, herança,
  parâmetro, tipo de retorno, campo, *cast*, `sizeof`, argumento de template,
  menção em `typedef`), `TypeUsageKind` cobre as espécies *de nível de
  assinatura* — `VariableDeclaration` (escopo de arquivo/namespace/membro
  estático), `Parameter`, `Field`, `ReturnType`, `Inheritance` e
  `TypedefMention` — e deixa de fora as de *nível de expressão* (`cast`,
  `sizeof`, `new`, argumento de template). O motivo é arquitetural, não
  preguiça de escopo: `extract_type_catalog` faz o parse com
  `CXTranslationUnit_SkipFunctionBodies` desde a correção de escala do
  Verovio 5.7.0 (ver "Escala" nas observações transversais), e ligar os corpos
  de função de volta só para este passo reintroduziria exatamente o custo que
  aquela correção eliminou. Essas espécies vivem dentro de corpos de função,
  então ficam de fora enquanto essa trade-off não for revisitada.
- **Resolvido: fonte da informação e custo.** `libclang`, reaproveitando a
  *mesma* varredura de AST que já constrói o catálogo de tipos (US-3) — nenhum
  segundo `clang_parseTranslationUnit` por projeto. `push_usage` roda dentro de
  `visit_cursor`/`record_member_dependency`, populando um terceiro vetor
  (`TypeCatalog.usages`) ao lado de `declarations`/`dependencies`. Isso também
  responde "o *scan* é feito antes": como é a mesma passada de US-3, os usos já
  estão persistidos no fim da criação do projeto, sem custo adicional
  perceptível.
- **Local do uso, não do tipo referenciado.** Cada `TypeUsage` aponta para a
  posição do *cursor que faz a referência* (o nome do campo, do parâmetro, da
  variável, da função, ou o especificador de base), não para o token do tipo
  em si — por exemplo, o uso de `Point` como tipo de campo em
  `Point origin;` fica na linha/coluna de `origin`, não de `Point`. Decisão
  deliberada: leva o usuário à declaração que usa o tipo (mais útil para
  navegação) em vez de só ao nome do tipo.
- **Segue em aberto: incrementalidade.** Como esta passada é a mesma de US-3, o
  índice de usos é recalculado por completo a cada criação de projeto, do
  mesmo jeito que o catálogo de tipos — reindexação por TU alterada continua
  sem solução, para os dois casos.
- **Segue em aberto: macros não geram uso.** Diferente do texto original ("têm
  precisão menor"), a decisão tomada aqui foi mais estrita: macros não entram
  na extração de usos (`TypeUsageKind` não tem variante para elas), porque
  correlacioná-las depende do *preprocessing record*, que este passe não
  consulta. Ficam de fora até esse trabalho ser feito, não apenas com menos
  precisão. Decisão: não abordado agora, ver "Preprocessing record de macros"
  nas observações transversais.
- **Segue em aberto: usos dentro de código não compilado.** Continua
  verdadeiro e sem solução — `#ifdef` desligado é invisível ao `libclang`.
- **Resolvido: cancelamento (critério 7).** Progresso já era relatado de
  graça, porque a indexação de usos anda junto da passada de tipos de US-3 e
  reaproveita os mesmos contadores atômicos que ela já expõe via
  `GET /projects/jobs/{id}`. Cancelamento reaproveita o mesmo job: um sinal
  compartilhado (`progress::Cancellation`, um `AtomicBool`) é checado uma vez
  por unidade de compilação dentro de `type_catalog::parse_chunk` e
  `source_catalog::parse_chunk` — best-effort, não preemptivo, então uma
  unidade já em processamento sempre termina antes do sinal ser observado.
  `DELETE /projects/jobs/{job_id}` pede o cancelamento e devolve `202` na
  hora, sem esperar o worker parar; `GET /projects/jobs/{job_id}` ganhou dois
  estados novos para refletir a diferença entre "pedido, ainda rodando"
  (`"cancelling"`) e "efetivamente parado" (`"cancelled"`) — sem essa
  distinção um poller veria `"running"` ficar preso depois de pedir o
  cancelamento, ou teria que inferir sucesso a partir da ausência de erro. O
  cliente Flutter ganhou um botão "Cancel" na tela de progresso
  (`creating_project_page.dart`) que chama a nova rota e mostra o resultado.
  Provado em três níveis: extração (`extract_type_catalog_stops_early_when_cancelled`,
  `extract_source_files_stops_early_when_cancelled` — um token
  pré-cancelado, determinístico, prova que o sinal é checado e para a
  pipeline, não só aceito como parâmetro), `create_project`
  (`create_project_reports_cancellation_and_persists_nothing` — cancelar
  antes de começar não persiste nada), e rota HTTP (dois testes
  determinísticos: id desconhecido devolve `404`; cancelar um job já
  terminado não sobrescreve um resultado real). Um quarto teste,
  `create_project_stops_within_seconds_of_cancellation_on_a_real_project`
  (`#[ignore]`d, ver "Condições de testabilidade"), prova cancelamento
  genuíno em pleno voo sobre o fixture Verovio real (298 unidades de
  compilação): cancelar ~500ms após iniciar interrompeu a extração em
  ~0.3s, contra os minutos que uma passada completa levaria.

### Critérios de aceitação (testáveis)

1. **Satisfeito** (para a taxonomia coberta): para um tipo do fixture com N
   usos conhecidos, o índice registra exatamente esses N locais, com arquivo,
   linha e coluna corretos. Provado por
   `extract_type_catalog_records_usages_across_the_defined_taxonomy`, que fixa
   um `Point`/`Widget` com um uso de cada uma das seis espécies e calcula a
   posição esperada a partir do próprio texto do fixture (não contada à mão).
2. **Satisfeito** (para a taxonomia coberta): cada uso é classificado segundo
   `TypeUsageKind`.
3. **Satisfeito:** a contagem exibida na lista (`usage_counts` em
   `GET /projects/types`, coluna "N uses"/"N use" em `TypesView`) é igual ao
   número de locais navegáveis persistidos.
4. **Satisfeito:** `TypesView` ordena por nome, por espécie e por número de
   usos, nos dois sentidos, com empate desfeito por nome em todos os casos.
5. **Satisfeito:** clicar em um uso no painel "Usages" abre o arquivo
   correspondente na linha correta (`UsagesView` → `_selectUsage` →
   `SourceFileViewer`).
6. **Satisfeito:** `GET /projects/types/usages?usr=…` responde a partir de
   `type_usages` já persistido, sem reparsear —
   `usages_route_returns_the_persisted_usages_for_a_type` popula o banco
   diretamente, sem `libclang`.
7. **Satisfeito:** projetos grandes (Verovio, ~290 TUs) são indexados com
   progresso real reportado (reaproveitando o contador de US-3), e
   cancelamento (`DELETE /projects/jobs/{job_id}`) interrompe a indexação em
   andamento — provado em pleno voo sobre o próprio fixture Verovio por
   `create_project_stops_within_seconds_of_cancellation_on_a_real_project`
   (`#[ignore]`d).

### Condições de testabilidade

- O fixture precisa ter contagens de uso *conhecidas e escritas no teste*, o
  que exige um fixture propositalmente pequeno e estável — mudanças nele
  quebram os testes, e isso é aceitável desde que ele seja versionado.
  **Satisfeito:** `USAGE_TAXONOMY_CPP` em `type_catalog.rs`, com localização
  calculada por busca textual (`locate_in`) em vez de contada à mão, para
  sobreviver a edições do fixture.
- Precisa existir um segundo fixture, maior, para testar progresso,
  cancelamento e tempo — com asserção sobre ordem de grandeza, não sobre
  duração exata, que não é reprodutível. **Parcialmente satisfeito:**
  `create_project_stops_within_seconds_of_cancellation_on_a_real_project`
  reaproveita o fixture Verovio 6.2.0 já versionado (298 unidades de
  compilação) para provar cancelamento em pleno voo, com asserção de ordem de
  grandeza (`elapsed < 60s` contra os minutos de uma passada completa) em vez
  de duração exata — mas, como a extração completa de um projeto desse
  tamanho leva minutos, o teste é `#[ignore]`d por padrão, mesmo precedente de
  `verovio_5_7_0_import_diagnosis.rs`. Ainda não há teste dedicado de
  *progresso* em escala especificamente para usos (o fixture Verovio de
  US-1/US-3 já prova que a extração não regride em escala, e o teste de
  cancelamento acima cobre esse ângulo, mas não há asserção sobre a forma dos
  contadores de progresso nesse fixture maior).
- Consultas de leitura precisam ser testáveis sem executar a indexação:
  popular o banco diretamente e consultar. **Satisfeito:**
  `usages_route_returns_the_persisted_usages_for_a_type` e os testes de
  round-trip em `project_store.rs` populam `type_usages` diretamente.

---

## US-5 — Funções, métodos e macros, e seus usos

**Status:** pronto — catalogação de funções/métodos/construtores/destrutores/
macros-função/templates, grafo de chamadas estático (com despacho dinâmico,
chamadas não resolvíveis e herança múltipla marcados) e navegação
definição↔chamadores nas duas direções existem e são testados. Seguem
deliberadamente em aberto, sem bloquear os próximos passos: incrementalidade
(mesma lacuna de US-3/US-4) e macros no grafo de chamadas (decisão explícita
de não abordar agora — ver "Preprocessing record de macros" nas observações
transversais) · **Depende de:** US-3 ·
**Implementação:** `crates/server/src/function_catalog.rs`
(`FunctionDeclarationKind::FunctionTemplate`, `overridden_usrs_of`,
`parameter_list` via *child-visiting*, resolução de chamada a template via
`clang_getSpecializedCursorTemplate`),
`crates/server/src/persistence/project_store.rs`
(tabelas `function_declarations`/`call_edges`,
`replace_function_declarations`/`list_function_declarations`,
`replace_call_edges`/`list_callers_for`/`list_calls_in_file`/`call_counts`,
coluna `overridden_usrs_json`, migração `migrate_function_columns`),
`crates/server/src/project_service.rs` (`FunctionCatalogListing`,
`list_functions`, `list_callers`, `list_calls_in_file`,
`CreationProgress::function_catalog`),
`crates/server/src/jobs.rs` (`JobPhase::CatalogingFunctions`, `derive_phase`),
`crates/server/src/server.rs` (`GET /projects/functions`,
`GET /projects/functions/callers`, `GET /projects/functions/calls-in-file`,
`function_catalog` em `GET /projects/jobs/{job_id}`),
`client/flutter/lib/src/project/project_models.dart`
(`FunctionDeclaration.overriddenUsrs`, `FunctionDeclarationKind.functionTemplate`,
`CallResolution`, `CallEdge`, `FunctionCatalogListing`,
`ProjectCreationJobPhase.catalogingFunctions`),
`client/flutter/lib/src/server/server_client.dart`/`http_server_client.dart`
(`listFunctions`, `listCallers`, `listCallsInFile`),
`client/flutter/lib/src/ui/functions_view.dart` (painel "Functions"),
`client/flutter/lib/src/ui/callers_view.dart` (painel "Callers"),
`client/flutter/lib/src/ui/source_file_viewer.dart` (`calls`/`onCallSelected`,
linha clicável quando há chamada registrada),
`client/flutter/lib/src/ui/server_status_page.dart` (`_selectCallTarget`,
`_loadCallsInFile`, fiação entre os painéis) ·
**Testes:** `crates/server/tests/function_catalog.rs`,
`crates/server/tests/function_catalog_route.rs`, testes inline em
`persistence/project_store.rs` e `jobs.rs`,
`client/flutter/test/functions_view_test.dart`,
`client/flutter/test/callers_view_test.dart`,
`client/flutter/test/source_file_viewer_test.dart`,
`client/flutter/test/app_test.dart`

### Objetivo do usuário

Identificar todas as funções, métodos e macros do projeto e todos os seus usos,
com a mesma navegação imediata de US-4, indo da definição ao uso e vice-versa.

### Observações e decisões em aberto

- **Resolvido: fonte da informação e custo — mas não do jeito cogitado.** A
  ideia original ("compartilha com US-4 a mesma infraestrutura de índice; duas
  passadas separadas seriam duplicação") só se sustentava em parte: o grafo de
  chamadas só existe dentro de corpos de função, e `type_catalog::
  extract_type_catalog` faz o parse com `CXTranslationUnit_SkipFunctionBodies`
  desde a correção de escala do Verovio 5.7.0 — reaproveitar aquela AST era
  literalmente impossível para este passo. `function_catalog::
  extract_function_catalog_cancellable` é portanto uma **terceira** passada
  completa de `libclang` sobre cada unidade de compilação (com corpos desta
  vez), independente das duas de US-3/US-4 — mesmo padrão de paralelização por
  núcleo (um `CXIndex` por worker) e o mesmo trade-off de custo já documentado
  em "Escala" abaixo. O que *foi* reaproveitado, como sugerido, é a
  infraestrutura de job/progresso/cancelamento de US-1/US-4: `CreationProgress`
  ganhou um terceiro `ExtractionProgress` (`function_catalog`) e
  `progress::Cancellation` já compartilhado é checado por esta passada também,
  então `DELETE /projects/jobs/{job_id}` interrompe as três passadas juntas.
- **Resolvido: taxonomia de declarações.** `FunctionDeclarationKind` cobre
  `FreeFunction`, `Method`, `Constructor`, `Destructor` e `FunctionMacro` — as
  quatro primeiras exigem `clang_isCursorDefinition` (mesma regra de US-3 para
  tipos: sem definição, sem entrada no catálogo; isso deixa construtores/
  destrutores/métodos *pure virtual* ou apenas declarados, sem corpo em
  lugar nenhum da TU, de fora — mesma lacuna documentada em US-3 como
  "forward declaration vs. definição", agora também para funções).
  `FunctionMacro` reaproveita a classificação já existente em
  `type_catalog::classify_macro` (US-3) em vez de reimplementá-la; as demais
  variantes de macro (`ConstantMacro`, `HeaderGuard`, `AnnotationMacro`) não
  são callables e continuam vivendo só no catálogo de tipos.
- **Resolvido: sobrecarga.** Cada `FunctionDeclaration` carrega `signature`
  (tipo de retorno, nome qualificado por namespace/classe, tipos e nomes de
  parâmetro, `const` quando aplicável) além do `usr` — os dois já distinguem
  duas sobrecargas do mesmo nome sem retrabalho futuro em US-7/US-8.
- **Resolvido: despacho dinâmico, via `clang_Cursor_isDynamicCall`.** Em vez de
  inferir despacho dinâmico por heurística (virtual + ausência de qualificador
  `Base::`), `libclang` expõe essa resposta diretamente para qualquer
  `CXCursor_CallExpr`. `CallResolution::Resolved.is_dynamic_dispatch` usa essa
  função; o `callee_usr` associado continua sendo o alvo estático (o método
  encontrado por *name lookup* no tipo estático do receptor), não o *overrider*
  real — coerente com o que `libclang` consegue saber sem executar o programa.
- **Resolvido: chamadas não resolvíveis.** `clang_getCursorReferenced` numa
  chamada indireta (ex.: através de ponteiro para função) resolve para a
  declaração da variável/parâmetro, não para uma função — `record_call`
  detecta isso (o cursor referenciado não é `FunctionDecl`/`CXXMethod`/
  `Constructor`/`Destructor`) e grava `CallResolution::Unresolved { reason }`
  em vez de omitir a chamada ou adivinhar um alvo.
- **Resolvido (parcial): método herdado não redefinido.** Como o catálogo só
  registra cursores de definição, um método que a classe derivada não
  redefine simplesmente não tem cursor próprio nela — só existe a definição na
  classe base, então a atribuição correta já sai de graça da mesma regra que
  resolve a taxonomia de declarações. Ainda não implementado: uma navegação
  explícita "ver todos os métodos herdados desta classe, redefinidos ou não"
  (mencionada no texto original) — hoje o usuário só vê o método listado sob a
  classe base.
- **Resolvido: definição ↔ chamadores (critério 5), nas duas direções.** A
  direção "da definição, listar chamadores" já estava completa
  (`ProjectStore::list_callers_for`/`CallersView`). A direção inversa — "de
  uma chamada, já visível no código fonte aberto, ir à definição" — ganhou
  `ProjectStore::list_calls_in_file` (espelha `list_callers_for`, mas
  filtrando por `file` em vez de `callee_usr`) e a rota
  `GET /projects/functions/calls-in-file`; `SourceFileViewer` recebe os
  `CallEdge`s do arquivo aberto e torna clicável qualquer linha com uma
  chamada registrada, chamando `_selectCallTarget` (em
  `server_status_page.dart`), que resolve o `callee_usr` no catálogo já
  carregado e reusa `_selectFunction` para navegar. **Simplificação
  deliberada:** a resolução não é precisa por coluna — `SourceFileViewer` não
  tem layout por token para *hit-test*, então uma linha com mais de uma
  chamada resolve para a primeira (por coluna), não para a mais próxima do
  clique. Prefere-se levar a *alguma* chamada da linha a não levar a
  nenhuma.
- **Resolvido: herança múltipla.** `first_overridden_usr` (que só guardava o
  primeiro cursor de `clang_getOverriddenCursors`) virou `overridden_usrs_of`,
  devolvendo todos os cursores sobrescritos — `FunctionDeclaration.overrides_usr:
  Option<String>` virou `overridden_usrs: Vec<String>`, persistido como
  `overridden_usrs_json` (mesmo padrão de `arguments_json` em
  `compilation_units`, já que herança múltipla é a única situação em que a
  lista passa de um elemento). Provado por
  `extract_function_catalog_records_every_overridden_base_under_multiple_inheritance`,
  com um `Square` que sobrescreve `area()` de duas bases não relacionadas
  (`Drawable`/`Measurable`).
- **Resolvido: sobrecarga de operadores, `inline`, `constexpr`, `template` e
  membros gerados pelo compilador.** Testado empiricamente com `libclang`
  (não presumido), a política ficou:
  - **Operadores, `inline`, `constexpr`:** já funcionavam — mesmo cursor
    `CXCursor_FunctionDecl`/`CXXMethod` que qualquer função/método, sem
    tratamento especial algum. O que parecia "não decidido" já era correto,
    só não estava confirmado.
  - **Templates (função e método):** eram invisíveis — o cursor
    `CXCursor_FunctionTemplate` não estava em `function_declaration_kind_for`.
    Ganharam `FunctionDeclarationKind::FunctionTemplate`, catalogando a
    declaração *primária* do template (não cada instanciação — monomorfização
    é decisão de US-7, listada lá). Descoberto no processo: `parameter_list`
    usava `clang_Cursor_getNumArguments`/`clang_Cursor_getArgument`, que não
    suportam `CXCursor_FunctionTemplate` (devolvem lista vazia
    silenciosamente) — reescrita para percorrer os filhos do cursor
    coletando `CXCursor_ParmDecl`, o que também corrigiu templates. E uma
    chamada a um template resolve, via `clang_getCursorReferenced`, para a
    *instanciação implícita*, cujo `usr` difere do da declaração primária que
    o catálogo guarda — `record_call` agora usa
    `clang_getSpecializedCursorTemplate` para mapear de volta à declaração
    primária, senão os chamadores de um template nunca apareceriam sob sua
    própria entrada no catálogo. Provado por
    `extract_function_catalog_lists_function_and_method_templates_by_their_primary_declaration`.
  - **Membros gerados pelo compilador** (ex.: construtor de cópia implícito):
    confirmado empiricamente que `libclang` **não emite cursor algum** para
    eles na travessia por filhos usada aqui, mesmo quando o membro é
    efetivamente usado (ODR-used) — não é "aparecem às vezes, dependendo do
    caso" como o texto anterior especulava; é "nunca aparecem, com esta API".
    Não há o que catalogar sem trocar de abordagem (ex.: forçar
    instanciação por outro mecanismo do `libclang`), o que fica fora de
    escopo — repetir esse achado aqui evita que um agente futuro gaste tempo
    reinvestigando o que já foi checado.
- **Ainda em aberto: incrementalidade.** Mesma lacuna de US-3/US-4 — a
  passada inteira é refeita a cada criação de projeto.
- **Ainda em aberto: macros não geram uso/chamada.** Uma macro-função aparece
  no catálogo (`FunctionMacro`), mas uma expansão de macro não vira uma aresta
  no grafo de chamadas — a expansão acontece no pré-processador, antes da AST
  que `visit_call_site` percorre, e correlacioná-la exigiria consultar o
  *preprocessing record* (mesma lacuna que US-4 já registrava para usos de
  tipo em macros). Decisão: não abordado agora, ver "Preprocessing record de
  macros" nas observações transversais.

### Critérios de aceitação (testáveis)

1. **Satisfeito:** o catálogo contém cada função livre, método, construtor,
   destrutor e macro-função do projeto, com assinatura completa. Provado por
   `extract_function_catalog_lists_every_callable_with_full_signature`.
2. **Satisfeito:** duas sobrecargas do mesmo nome aparecem como entradas
   distintas (usr e assinatura diferentes) — mesmo teste, par `add(int, int)`/
   `add(double, double)`.
3. **Satisfeito:** uma chamada a método virtual através de referência à classe
   base é registrada e marcada como despacho dinâmico
   (`clang_Cursor_isDynamicCall`). Provado por
   `extract_function_catalog_records_the_call_graph_with_libclang`
   (`describe(const Shape&)` chamando `shape.area()`).
4. **Satisfeito:** um método herdado e não redefinido é atribuído à classe que
   o define (`Circle` não redefine `perimeter`, que aparece só sob `Shape`) —
   mesmo teste do critério 1.
5. **Satisfeito:** de uma função é possível listar seus chamadores
   (`list_callers_for`/`GET /projects/functions/callers`/`CallersView`), e
   clicar num chamador abre o arquivo na linha exata da chamada. Na direção
   inversa, clicar numa chamada já visível num arquivo fonte aberto pula
   para a definição do que ela chama
   (`list_calls_in_file`/`GET /projects/functions/calls-in-file`/
   `SourceFileViewer.onCallSelected`) — com a simplificação de coluna
   descrita na observação acima quando há mais de uma chamada na mesma
   linha. Provado por
   `calls_in_file_route_returns_the_persisted_calls_for_a_file`, pelos
   testes de `SourceFileViewer` em `source_file_viewer_test.dart`, e
   ponta a ponta por `app_test.dart` (clicar a chamada de `area` dentro de
   `describe` abre `shapes.h` e popula o painel "Callers" para `area`).
6. **Satisfeito:** uma chamada não resolvível estaticamente (aqui, através de
   ponteiro para função) aparece marcada como tal
   (`CallResolution::Unresolved`), não omitida — mesmo teste do critério 3
   (`apply`'s `op(x, y)`).

### Condições de testabilidade

- **Satisfeito:** `FUNCTIONS_CPP` em `crates/server/tests/function_catalog.rs`
  contém, deliberadamente: uma hierarquia (`Shape`/`Circle`) com um método
  virtual redefinido (`area`) e outro não redefinido (`perimeter`), um par de
  sobrecargas (`add(int, int)`/`add(double, double)`), um ponteiro para função
  (`BinaryOp`, chamado indiretamente dentro de `apply`) e uma macro-função
  (`SQUARE`).
- **Satisfeito:** os números esperados de chamadores são pequenos e escritos à
  mão no teste (um chamador para `add(int, int)`, um para `area`, uma chamada
  não resolvível dentro de `apply`).
- **Satisfeito:** herança múltipla e templates usam fixtures próprios,
  separados de `FUNCTIONS_CPP`, para não perturbar as contagens exatas que
  esse primeiro já fixa — mesma razão pela qual US-3 mantém o teste de
  homônimos em namespace separado do fixture combinado.
- Rotas de leitura (`GET /projects/functions`, `GET /projects/functions/
  callers`, `GET /projects/functions/calls-in-file`) são testadas sem
  executar `libclang`, populando o banco diretamente — mesmo padrão de
  `type_catalog_route.rs` (`crates/server/tests/function_catalog_route.rs`).
- **Satisfeito:** testes de widget de `SourceFileViewer` populam `calls`
  diretamente (sem servidor real) para provar o clique-para-navegar de forma
  determinística, isolada da complexidade de acordeões/painéis dockados da
  página inteira — que por sua vez exigem `tester.ensureVisible` antes do
  `tap` nos testes de ponta a ponta em `app_test.dart`, já que um painel
  dockado ao centro pode empurrar a linha clicada para fora da área visível.

---

## US-6 — Isolamento e caracterização comportamental

**Status:** planejado — **fase A destravada** pelas respostas da rodada 1 (não
depende de KLEE, GoogleTest nem isolamento); fase B **adiada** por decisão
explícita (Q10) · **Depende de:** US-5

Este passo absorve o conteúdo do antigo plano de separação de unidades de
compilação, que era mantido em documento próprio.

### US-6 é opcional de ponta a ponta

**Decisão do usuário, e ela vale para US-6 inteiro, não só para a escolha de
quais funções caracterizar.** O Syntax Bridge deve *oferecer as ferramentas* de
caracterização comportamental, mas o usuário pode escolher simplesmente não
executá-las e ainda assim converter o projeto. Caracterização é instrumento de
confiança, não pedágio do fluxo de conversão.

Isso vai além de OBS 1: OBS 1 dizia que a caracterização é *seletiva* (o usuário
escolhe quais funções); esta decisão diz que ela é *dispensável* (o usuário pode
escolher nenhuma, inclusive nunca abrir a tela). As consequências atravessam
vários passos:

- **Nenhum passo posterior pode ter US-6 como pré-requisito duro.** US-7, US-8,
  US-9 e US-11 precisam funcionar com zero traces gravados. Um projeto sem
  caracterização alguma é um estado normal do produto, não um estado incompleto,
  e não pode produzir erro, aviso repetido nem bloqueio de exportação.
- **US-10 é o passo que mais sente, e já estava preparado.** Ele depende de "uma
  fonte de oráculo", que pode ser a fase A de US-6 *ou* os casos escritos à mão
  da escada de exemplos — nunca de US-6 obrigatoriamente. Sem oráculo algum,
  US-10 não roda, e o produto **reporta cobertura de prova zero** em vez de
  fingir sucesso: é a mesma regra de "silêncio é proibido" da receita padrão.
- **Exportar sem prova é um caminho legítimo** (US-11), desde que o relatório
  diga com clareza o que não foi provado. A decisão de exportar código não
  verificado é do usuário; a de escondê-lo dele, nunca.
- **Custo de caracterizar precisa ser visível antes de ser pago.** Como US-6
  compila o projeto de entrada (Q7) e roda execuções reais, e isso leva minutos
  num projeto do tamanho do Verovio, o usuário só decide bem se souber o preço
  antes de entrar — daí o mecanismo de job (progresso + cancelamento) valer aqui
  tanto quanto em US-1.
- **Na UI**, isso significa que os painéis de caracterização são uma área que o
  usuário visita se quiser, e o fluxo principal (catálogos → mapeamento →
  geração → exportação) não passa por dentro deles.

### Notas do revisor sobre OBS 1 e OBS 2 (ler antes das duas)

> Esta seção é o **registro histórico** da rodada 1 de revisão. As quatro
> perguntas que ela levanta foram todas respondidas — as respostas e as
> consequências estão no fim dela, e já estão incorporadas aos sub-passos. Quem
> quiser só o resultado pode pular direto para "As duas fases, e por que a ordem
> importa".

**Inconsistência encontrada e corrigida:** o índice no topo do documento já
classificava US-6 como `planejado`, mas o corpo deste item dizia
`Status: adiado` — um rótulo que nem existe na taxonomia definida em "Como ler
cada passo" (`pronto`/`parcial`/`planejado`). Corrigido acima. `adiado` parecia
ser um jeito informal de dizer "bloqueado por KLEE/GoogleTest ausentes do
Flatpak" (já registrado em "Observações transversais → Ambiente de teste"). Vale
manter essa constatação porque o desdobramento abaixo mostra que nem tudo em
US-6 está de fato bloqueado por essa ausência — só a via que depende de KLEE.

**Sobre OBS 1 (passo opcional, escolhido pelo usuário):** isso corrige uma
premissa que os critérios de aceitação antigos carregavam implicitamente — eles
falavam em "uma função pura simples do fixture" como se a caracterização fosse
automática/global em vez de seletiva. Com OBS 1, faltam três decisões que o
texto original não tinha: onde o usuário faz a escolha (provável extensão do
painel "Functions" de US-5, não uma tela nova), como a escolha é persistida, e
o que acontece quando a função escolhida chama outra que não foi escolhida — a
seleção não elimina o problema de granularidade do isolamento já registrado
abaixo (agora US-6.4), só decide *quais* funções entram na fila. Virou **US-6.1**.

**Sobre OBS 2 (ferramenta de log estruturado):** este é o ponto que mais precisa
de decisão sua, porque a nota admite duas leituras que levam a arquiteturas bem
diferentes e o texto não deixa claro qual é a pretendida:

1. É a instrumentação injetada no código *isolado* — o que a frase "a função em
   questão, instrumentada" do objetivo original já previa. Complementa KLEE, não
   o substitui.
2. É uma instrumentação injetada no código *original*, para captar comportamento
   de execuções reais (a suíte de testes que o projeto C++ de entrada já tiver,
   por exemplo via GoogleTest, ou uma sessão manual do usuário). Isso evitaria,
   para um primeiro incremento, todo o problema de *program slicing*/mocks —
   e destravaria boa parte de US-6 sem depender de KLEE, que hoje não está no
   Flatpak.

A leitura 1 dá cobertura de ramos "de graça" via busca de entradas, mas herda
tudo que já está em aberto sobre isolamento e a ausência do KLEE. A leitura 2 é
mais barata e não depende de ferramenta ausente, mas só prova o que as
execuções reais exercitarem — a cobertura deixa de ser garantida e precisa ser
medida e reportada como parcial. Registrei as duas como uma decisão explícita em
**US-6.3**, com uma pergunta direta abaixo. Isso também é o motivo de este item
ter sido desdobrado: sem separar "o que caracterizar" (US-6.1), "o que gravar"
(US-6.2), "de onde vêm as execuções" (US-6.3), "como isolar, se necessário"
(US-6.4) e "onde e com que limites o resultado vive" (US-6.5), a decisão sobre
OBS 2 fica implícita dentro de um único item grande demais para revisar aos
poucos.

**Perguntas para revisão — respondidas.** As respostas estão preservadas
literalmente; cada uma é seguida da consequência que ela produz nos sub-passos.

1. Em OBS 1, a seleção é só de funções "folha" (que não chamam outras
   não selecionadas), ou qualquer função — aceitando que suas dependências
   sejam automaticamente incluídas/mockadas pelo mecanismo de isolamento
   (US-6.4)?

   Resposta: Permitir as duas coisas: Somente a função e a função e tudo o que ela chama.

   **Consequência:** a seleção não é um booleano, é um par
   `(função, escopo)` com dois escopos — `FunctionOnly` e `FunctionAndCallees`.
   O segundo é o **fecho transitivo** sobre o grafo de chamadas de US-5, que
   já existe persistido (`call_edges`) e portanto não custa nada extrair de
   novo. Decisão de projeto derivada: persistir apenas a *raiz* da seleção e
   derivar o fecho a cada leitura, nunca materializá-lo — assim uma mudança no
   código (US-12) ou uma chamada nova recalculam o conjunto sozinhas, em vez de
   deixarem uma lista salva desatualizada. Detalhado em US-6.1.

2. Em OBS 2, a leitura pretendida é (a) instrumentação do código isolado
   gerado a partir de entradas sintéticas, (b) instrumentação do código
   original rodando com testes/uso reais, ou (c) as duas, em fases
   diferentes? A resposta decide se US-6.3/US-6.4 (isolamento, KLEE) precisam
   estar prontos antes do primeiro incremento entregável de US-6, ou não.

   Resposta: opção c.

   **Consequência:** US-6 passa a ter duas fases explícitas, e a **fase A não
   depende de KLEE nem de isolamento**:
   - **Fase A — execução real instrumentada.** Instrumenta o código original,
     roda o programa como ele é, grava o comportamento observado. Depende só de
     `clang++` e `cmake`, ambos já no Flatpak. É o primeiro incremento
     entregável de US-6 e destrava US-10 sem esperar ferramenta nenhuma.
   - **Fase B — entradas sintéticas sobre código isolado.** Instrumenta o
     código isolado de US-6.4, com entradas geradas por KLEE. Depende de
     US-6.4 e de KLEE — GoogleTest já está disponível no ambiente Flatpak
     (revisão de Q10, ver AGENTS.md), mas isso sozinho não destrava a fase B:
     GoogleTest materializa e executa casos, quem os descobre é KLEE.
   A instrumentação de US-6.2 é a **mesma** nas duas fases — muda o que é
   compilado e quem produz as entradas, não o que é gravado. Isso é o que
   permite que a fase B substitua a fase A sem trocar o formato do registro,
   exatamente como `conversao-guiada-por-exemplos.md` §9 exige do formato de
   `oracle/cases.json`. US-6.4 deixa de ser bloqueante para US-6 inteiro e
   passa a ser bloqueante só da fase B.

3. Ainda em OBS 2: a granularidade da escolha é por tipo (toda struct `X`
   sempre gravada onde aparecer) ou por local de uso (só esta variável, nesta
   função)? E o padrão é gravar tudo e o usuário restringe (*opt-out*), ou
   nada até ele marcar (*opt-in*)?

   Resposta: O 'gravar/não gravar' é controlado de dentro de uma função. É nas funções que
   se decide se uma instância de uma struct será ou não gravada no log.

   **Consequência:** a unidade de escolha é o par
   `(função selecionada, entidade dentro dela)` — um parâmetro, uma variável
   local, o retorno, `this`, um global lido/escrito ali — e **não** o tipo. Dois
   desdobramentos concretos:
   - É **opt-in** por construção: não existe "gravar tudo" a partir de dentro de
     uma função; só existe marcar. Nada é gravado até haver marcação, o que
     também mantém o critério 3 de US-6.1 (nenhum artefato para função não
     selecionada) trivialmente satisfeito.
   - A UI da marcação é o **visualizador de código fonte** com a função aberta,
     não uma tela de tipos: o usuário marca onde a entidade aparece. Isso exige
     do servidor uma lista de *entidades capturáveis por função*, que hoje não
     existe — o passe de US-5 já parseia corpos de função (é o único que
     parseia), mas só registra parâmetros e chamadas. Detalhado em US-6.2 como
     a primeira tarefa daquele sub-passo, e explicitamente **sem quarta passada
     `libclang`**.

4. Se a resposta a (2) for (b) ou (c): os projetos de entrada que o produto
   precisa suportar costumam já trazer sua própria suíte de testes (GoogleTest
   ou outra)? Se sim, rodar essa suíte já existente sob instrumentação parece
   o caminho mais barato para um primeiro oráculo de US-10, sem esperar KLEE —
   vale registrar isso como o incremento inicial de US-6.3 se confirmado.

   Resposta: O projeto pode não trazer suíte alguma de teste. No meu caso, o Verovio não possui suíte de testes.

   **Consequência — a mais pesada das quatro.** O caminho mais barato que a
   pergunta cogitava (rodar a suíte existente sob instrumentação) **não está
   disponível** no caso de referência do produto. A fase A precisa então de
   outra fonte de execuções, e a que o Verovio oferece é a óbvia: o projeto
   produz um **executável**, e rodá-lo sobre um arquivo de entrada real
   (`verovio partitura.mei -o saida.svg`) exercita o código de verdade. Isso
   introduz um conceito que o documento não tinha: o **perfil de execução** —
   alvo executável, argumentos, arquivos de entrada, diretório de trabalho e
   código de saída esperado — configurado pelo usuário e persistido no projeto.
   Duas consequências de segunda ordem, ambas novas:
   - O produto passa a **compilar** o projeto de entrada. Até US-5 ele apenas
     *configura* o CMake (`CMAKE_EXPORT_COMPILE_COMMANDS`) e lê
     `compile_commands.json`; nunca chamou `cmake --build`. Ver Q7 (respondida:
     compila só os alvos necessários, dentro do mecanismo de job).
   - A cobertura deixa de ser garantida (é o que a via real cobra em troca de
     não depender de KLEE), mas **é medível sem KLEE**: `clang++` com
     `-fprofile-instr-generate -fcoverage-mapping` mais `llvm-profdata`/`llvm-cov`,
     tudo já disponível pela extensão `llvm21` do manifesto. Isso satisfaz o
     critério 2 de US-6.3 ("medida e reportada, nunca presumida") na fase A.
   - Uma suíte de testes do projeto de entrada, quando existir, vira apenas
     *mais um perfil de execução*, não um caminho de código próprio. Essa é a
     forma de acomodar as duas realidades sem duas implementações.

### Objetivo do usuário

Que o Syntax Bridge documente, por execução real, como cada função escolhida
pelo usuário se comporta, e grave no banco o comportamento observado (entradas,
resultado, coleções modificadas, efeitos) de acordo com os dados que o próprio
usuário marcar para captura. Este objetivo geral é realizado pelos cinco
sub-passos abaixo.

### As duas fases, e por que a ordem importa

Decorrência da resposta 2 (opção "c"). Um agente que for implementar US-6 deve
entregar a fase A inteira antes de tocar em qualquer parte da fase B.

| | Fase A — execução real | Fase B — entradas sintéticas |
| --- | --- | --- |
| O que é instrumentado | o código original do projeto | o código isolado de US-6.4 |
| De onde vêm as execuções | perfis de execução do usuário (US-6.3) | KLEE (US-6.3) |
| Cobertura | medida por `llvm-cov`, parcial e reportada | buscada por ramo, ainda assim medida |
| Ferramentas | `cmake`, `clang++`, `llvm-cov` — **todas no Flatpak hoje** | KLEE (fora) + GoogleTest (**já no Flatpak**, revisão de Q10) |
| Sub-passos envolvidos | US-6.1, US-6.2, US-6.3(A), US-6.5 | US-6.3(B), US-6.4 |
| Destrava | US-10 com oráculo real | cobertura de ramos garantida |

O que as duas fases **compartilham** é o que não pode divergir: a seleção
(US-6.1), a marcação de captura e o formato do registro (US-6.2), e o esquema
de persistência (US-6.5). Se a fase B precisar mudar o formato do registro, o
formato foi mal projetado na fase A — mesmo critério que
`conversao-guiada-por-exemplos.md` §9 aplica ao `oracle/cases.json`.

### Ordem de implementação recomendada

1. US-6.1 (seleção) — puro CRUD sobre dado já existente, sem toolchain.
2. US-6.2, parte 1 (listar entidades capturáveis por função) — extensão do
   passe de US-5, sem passe novo.
3. US-6.2, parte 2 (marcação de captura) — CRUD, mesma forma de US-6.1.
4. US-6.5, parte 1 (esquema de `characterization_runs`/`behavior_traces`) —
   antes de gerar qualquer trace, para que o gerador já escreva no formato final.
5. US-6.2, parte 3 (emissão da instrumentação + runtime de trace em C++).
6. US-6.3 fase A (perfis de execução, build instrumentado, coleta, cobertura).
7. US-6.5, parte 2 (limites de execução, determinismo, segurança).
8. US-6.4 e US-6.3 fase B — **fora da v1** por Q10; só voltam à fila se a
   cobertura medida na fase A justificar KLEE no manifesto.

### OBS 1 (original). Passo opcional, de acordo com escolha do usuário.
O objetivo é extrair comportamento do código, mas não precisamos fazer isto para TUDO. As funções/métodos/trechos do código que passarão por isto serão escolhigos pelo usuário.

### OBS 2 (original). Ferramenta de geração de 'log' estruturado.
Gerar código que grave os dados de variáveis, conteúdo de structs, collections. O usuário deverá ser capaz de definir quais dados/classes/variáveis/structs/collections serão gravados.

---

#### US-6.1 — Seleção do escopo de caracterização

**Status:** planejado · **Depende de:** US-5

##### Objetivo do usuário

Escolher quais funções/métodos serão caracterizados, em vez de o sistema tentar
caracterizar tudo. Nada é caracterizado até o usuário selecionar algo.

##### Observações e decisões em aberto

- **Resolvido (resposta 1): dois escopos de seleção, não um booleano.**
  `SelectionScope::FunctionOnly` seleciona exatamente aquela função;
  `SelectionScope::FunctionAndCallees` seleciona também o **fecho transitivo**
  das funções que ela chama, sobre o grafo `call_edges` já persistido em US-5.
- **Só a raiz é persistida; o fecho é derivado a cada leitura.** Materializar o
  fecho no banco criaria uma cópia que envelhece: bastaria o usuário adicionar
  uma chamada no código para a lista salva ficar errada, e US-12
  (re-ingestão) teria que reconciliá-la. Derivar é barato — é uma travessia de
  um grafo já indexado — e sempre correto.
- **O fecho precisa de três defesas, todas testáveis.** (a) Ciclos: recursão
  direta e mútua existem em código real, então a travessia usa conjunto de
  visitados. (b) Chamadas não resolvíveis: um `CallResolution::Unresolved` de
  US-5 é uma fronteira do fecho que não dá para atravessar — deve ser
  **reportada** como fronteira incompleta, com origem e motivo, nunca
  silenciosamente ignorada (regra "silêncio é proibido"). (c) Despacho
  dinâmico: `call_edges` guarda o alvo *estático*; num fecho, um método virtual
  deveria puxar também os *overriders*, que `FunctionDeclaration.overridden_usrs`
  permite descobrir na direção inversa. Decisão: incluir os *overriders* no
  fecho e marcá-los como "incluídos por despacho dinâmico", porque excluí-los
  produziria caracterização de um comportamento que o programa real não tem.
- **Explosão do fecho é um risco real, não teórico.** Selecionar
  `FunctionAndCallees` numa função de alto nível do Verovio pode arrastar
  centenas de funções. A listagem devolvida ao cliente precisa informar o
  tamanho do fecho *antes* de o usuário confirmar, e a UI precisa mostrar esse
  número — não é uma otimização, é o que impede o usuário de disparar sem saber
  uma instrumentação de projeto inteiro.
- **Desmarcar não apaga resultado (critério 4).** Traces já gravados para uma
  função que saiu do conjunto efetivo ficam marcados como órfãos, não
  removidos: apagar é irreversível e cara de refazer (exige recompilar e
  reexecutar), enquanto marcar é barato e é exatamente a mesma mecânica que
  US-12 vai precisar para "decisão que deixou de ser válida".
- Onde a seleção acontece na UI: extensão do painel "Functions" (US-5) com uma
  ação por linha, não uma tela nova.
- Relação com US-12 (re-ingestão): uma função selecionada cujo código muda
  precisa reaparecer como "selecionada, mas pendente de recaracterização" —
  mesma mecânica já prevista para decisões de US-7.

##### Critérios de aceitação (testáveis)

1. Selecionar N funções no fixture persiste exatamente essas N seleções.
2. Reabrir o projeto preserva a seleção sem re-executar nada.
3. Nenhum artefato de caracterização (código isolado, trace, registro no banco)
   é gerado para uma função não selecionada.
4. Desmarcar uma função remove sua seleção; traces já gravados para funções que
   saíram do conjunto efetivo ficam marcados como órfãos, não apagados.
5. Selecionar com escopo `FunctionAndCallees` o topo de uma cadeia de três
   níveis produz um conjunto efetivo com as três funções; com escopo
   `FunctionOnly`, com uma só.
6. Uma cadeia com recursão (direta e mútua) produz fecho finito, sem laço.
7. Uma chamada não resolvível dentro do fecho aparece na resposta como
   fronteira incompleta, com arquivo, linha e motivo.
8. Um método virtual dentro do fecho arrasta seus *overriders*, marcados como
   incluídos por despacho dinâmico.

##### Condições de testabilidade

- Rotas de leitura/escrita da seleção testáveis sem executar `libclang` nem
  KLEE — popular o banco diretamente, mesmo padrão de US-3/US-4/US-5.
- Fixture com uma cadeia de chamadas de pelo menos três níveis, para exercitar
  o caso "selecionei o meio da cadeia". Como os critérios 6, 7 e 8 pedem
  recursão, chamada indireta e hierarquia virtual, e como misturar tudo num
  fixture só tornaria as contagens exatas frágeis, valem fixtures separados —
  mesma razão já aplicada em US-5 para herança múltipla e templates.
- A derivação do fecho precisa ser uma função pura sobre
  `(seleções, call_edges, declarações)`, testável sem banco e sem servidor.
  Esse é o teste que importa; os demais níveis são encanamento.

##### Roteiro de implementação (para um agente)

Segue a receita padrão. Nenhum item aqui precisa de `libclang`, `cmake` ou
qualquer toolchain — é o sub-passo mais barato de US-6 e por isso vem primeiro.

1. **`crates/server/src/characterization.rs`** (módulo novo, registrado em
   `lib.rs`): `SelectionScope { FunctionOnly, FunctionAndCallees }`,
   `CharacterizationSelection { function_usr, scope }`, e
   `EffectiveSelection { functions: Vec<SelectedFunction>, boundary: Vec<UnresolvedBoundary> }`,
   onde `SelectedFunction` carrega `usr`, `signature` e `InclusionReason`
   (`Root` | `Callee { via_usr }` | `DynamicOverride { of_usr }`).
2. **Teste primeiro**, em `crates/server/tests/characterization.rs`:
   `effective_selection_expands_callees_transitively`,
   `effective_selection_terminates_on_recursive_cycles`,
   `effective_selection_reports_unresolved_calls_as_boundary`,
   `effective_selection_includes_dynamic_overriders`. Todos alimentam a função
   de derivação com vetores de `CallEdge`/`FunctionDeclaration` montados à mão
   — sem banco, sem parser.
3. **Derivação:** `characterization::derive_effective_selection(...)`, travessia
   em largura com `HashSet` de visitados, ordenação final estável por `usr`
   (senão os testes ficam intermitentes e o critério de determinismo de US-6.5
   já nasce quebrado).
4. **Persistência** em `project_store.rs`: tabela
   `characterization_selections (function_usr TEXT PRIMARY KEY, scope TEXT NOT NULL)`,
   com `replace_characterization_selections`/`list_characterization_selections`
   e teste inline de round-trip. Tabela nova, então não há migração a fazer.
5. **Serviço** em `project_service.rs`: `list_characterization_selections`
   devolvendo um `CharacterizationSelectionListing` que já traz o conjunto
   efetivo derivado e o tamanho do fecho; `set_characterization_selection` e
   `clear_characterization_selection`.
6. **Rotas** em `server.rs`: `GET /projects/characterization/selections`,
   `PUT /projects/characterization/selections`,
   `DELETE /projects/characterization/selections`. Teste em
   `crates/server/tests/characterization_route.rs`, populando o banco direto.
7. **Cliente:** modelos em `project_models.dart`, métodos em
   `server_client.dart`/`http_server_client.dart`.
8. **UI:** em `functions_view.dart`, um controle por linha com três estados
   (não selecionada / só esta / esta e o que ela chama) e, quando o escopo for
   o segundo, o tamanho do fecho ao lado. Painel novo
   `characterization_view.dart` listando o conjunto efetivo, com as fronteiras
   incompletas visíveis. Teste de widget próprio, mais a fiação em
   `server_status_page.dart` e o caso ponta a ponta em `app_test.dart`.

---

#### US-6.2 — Captura estruturada de dados (instrumentação)

**Status:** planejado · **Depende de:** US-6.1

##### Objetivo do usuário

Para uma função selecionada, escolher — de dentro dela, no código fonte —
quais parâmetros, variáveis locais, campos e coleções são gravados durante a
execução, e obter um registro estruturado desses valores.

##### Observações e decisões em aberto

- **Resolvido (resposta 2): a instrumentação atua sobre os dois códigos, em
  fases.** Fase A: o código original do projeto. Fase B: o código isolado de
  US-6.4. O emissor de instrumentação e o *runtime* de trace são os mesmos nas
  duas — muda apenas qual árvore de arquivos é instrumentada. Portanto **este
  sub-passo não depende de US-6.3 nem de US-6.4** e pode ser implementado
  logo depois de US-6.1.
- **Resolvido (resposta 3): a unidade de escolha é `(função, entidade)`, e é
  *opt-in*.** Não existe "marcar o tipo `Point` em todo lugar"; existe "marcar
  o parâmetro `origin` desta função". Consequência de projeto: o mesmo tipo
  pode ser gravado numa função e ignorado noutra, o que é o comportamento
  pedido, e mantém o custo da instrumentação proporcional ao que o usuário
  marcou, não ao tamanho do projeto.
- **Falta um catálogo de *entidades capturáveis*, e ele é a primeira tarefa
  deste sub-passo.** Para o usuário marcar de dentro da função, o servidor
  precisa saber o que existe dentro dela: parâmetros (já extraídos por
  `parameter_list` em US-5), variáveis locais (`CXCursor_VarDecl` no corpo —
  **não** extraídas hoje), o valor de retorno, `this` quando for método, e
  globais lidos/escritos ali. Isso **não exige uma quarta passada `libclang`**:
  `function_catalog::extract_function_catalog_cancellable` já é a única passada
  que parseia corpos de função (as de US-3/US-4 usam
  `CXTranslationUnit_SkipFunctionBodies`), então a coleta entra na mesma
  travessia que já monta o grafo de chamadas — mesmo aproveitamento que US-4
  fez sobre a passada de US-3.
- **Cada entidade capturável carrega o `usr` do tipo dela**, ligando ao catálogo
  de US-3. É esse elo que permite ao emissor saber *como* serializar (agregado,
  escalar, ponteiro, contêiner) e, mais tarde, permite a US-10 comparar valores
  entre C++ e Dart usando o mapeamento decidido em US-7.
- **Política de serialização — declarada, não implícita.** Sem isto o critério 2
  de US-6.5 (duas execuções iguais produzem o mesmo registro) é impossível:
  - **Endereços nunca são gravados.** Um ponteiro vira `null`, ou o valor
    apontado, ou uma referência de volta (`{"ref": n}`) quando já visitado.
    Gravar endereço destrói o determinismo — ASLR muda o valor a cada execução.
  - **Ciclos** são cortados por um mapa de identidade durante a serialização,
    emitindo a referência de volta em vez de recursão infinita.
  - **Profundidade máxima de travessia: 3 níveis** por padrão, configurável por
    projeto. Ao cortar, o registro diz que cortou (`{"truncated": "depth"}`).
  - **Coleções: 64 primeiros elementos** por padrão, mais o tamanho total, mais
    um *hash* estável do conteúdo completo. O hash é o que mantém uma coleção
    truncada ainda comparável em US-10 — sem ele, truncar destrói o oráculo.
  - **Ponto flutuante** é gravado em duas formas: decimal canônico e os bits
    crus. A comparação entre linguagens (US-10) precisa dos bits; o usuário
    lendo o trace precisa do decimal.
  - **Ponteiro pendente (*dangling*) é indetectável** e vai continuar sendo —
    a instrumentação não tem como distingui-lo de um ponteiro válido. Fica
    registrado aqui como limitação conhecida, não como pendência.
  - Todo registro carrega `schema_version`. A fase B não deve precisar
    incrementá-la; se precisar, o formato foi mal projetado.
- **Resolvido (Q5): a instrumentação entra por reescrita de uma cópia do
  fonte** — opção (a), a recomendada. O produto gera uma árvore instrumentada em
  `<projeto>/characterization/instrumented-source/`, guiada pelas posições de
  cursor que os catálogos de US-3/US-5 já registram, e `input-source` permanece
  intocado. As alternativas descartadas eram (b) *wrappers* gerados que chamam a
  função original e (c) um header injetado por `-include` com macros; ambas são
  mais baratas, mas nenhuma alcança variável local *dentro* do corpo, então
  escolher qualquer uma delas revogaria na prática a resposta 3 da rodada 1, que
  já decidiu que variáveis locais são capturáveis. (a) também não muda o
  comportamento por interposição e mantém o original imutável — o que atende ao
  critério "nada escreve fora do diretório do projeto" de US-6.5. **Custo
  aceito:** o produto precisa de um reescritor de fonte guiado por posição de
  cursor, que ainda não existe, e ele é pré-requisito do item 5 do roteiro
  abaixo.

##### Critérios de aceitação (testáveis)

1. Para uma função do fixture, o catálogo de entidades capturáveis lista seus
   parâmetros, suas variáveis locais, seu retorno e — se for método — `this`,
   cada um com o `usr` do respectivo tipo.
2. Para uma função com uma struct e uma coleção como parâmetros, marcar ambas
   produz um registro com o conteúdo de ambas após a execução.
3. Uma entidade não marcada não aparece no registro.
4. Uma coleção com mais elementos que o limite é gravada truncada, com tamanho
   total e hash do conteúdo completo, sem estourar tempo nem memória.
5. Um ponteiro nulo é registrado como `null`, sem falha da instrumentação.
6. Um grafo de objetos com ciclo é serializado com referência de volta, e a
   serialização termina.
7. Duas execuções do mesmo binário sobre a mesma entrada produzem registros
   idênticos byte a byte (nenhum endereço, nenhum timestamp no payload).

##### Condições de testabilidade

- Os critérios 1 e 3 são testáveis sem compilar nada: o primeiro é extração
  (`libclang`), o terceiro é inspeção do código instrumentado gerado, que pode
  ser comparado com um *golden* em texto.
- Os critérios 2, 4, 5, 6 e 7 exigem compilar e executar — `clang++` já está no
  Flatpak, então isso é testável no ambiente de destino desde já, sem esperar
  US-6.3 (um `main` mínimo escrito pelo teste basta como driver).
- Fixture com struct aninhada, coleção (`std::vector`/`std::map`), um ponteiro
  nulo deliberado e uma estrutura com ciclo (dois nós que se apontam).
- O *runtime* de trace precisa ser um artefato versionado e lido por humanos
  (`crates/server/resources/trace_runtime/syntax_bridge_trace.hpp`), não uma
  string embutida no meio do gerador — senão ninguém consegue revisá-lo.

##### Roteiro de implementação (para um agente)

1. **Entidades capturáveis (extração).** Teste primeiro em
   `crates/server/tests/function_catalog.rs`:
   `extract_function_catalog_lists_capturable_entities_of_each_function`. Depois
   estender `function_catalog.rs` com `CapturableEntity { function_usr, kind,
   name, type_usr, line, column }` e `CapturableEntityKind { Parameter, Local,
   Return, This, Global }`, coletada na travessia que já existe (`visit_call_site`
   e vizinhança), sem passe novo. Persistir em tabela `capturable_entities`
   (receita padrão, passo 4) e expor via
   `GET /projects/functions/capturable-entities`.
2. **Marcação (CRUD).** Tabela
   `capture_marks (function_usr, entity_key, moments TEXT)`, com `moments` em
   `entry`/`exit`/ambos. Rotas `GET`/`PUT`/`DELETE
   /projects/characterization/captures`. Mesma forma de US-6.1, testes de rota
   populando o banco direto.
3. **Esquema do trace antes do gerador.** Implementar aqui a parte 1 de US-6.5
   (tabelas `characterization_runs` e `behavior_traces`), para que o emissor já
   nasça escrevendo no formato definitivo.
4. **Runtime de trace em C++.** Header versionado, com testes próprios: um
   `main` de teste que serializa os casos difíceis (ciclo, coleção grande,
   ponteiro nulo, float) e compara com *golden*. Este é o componente que
   precisa ser mais bem testado de US-6 inteiro, porque um bug aqui contamina
   silenciosamente todo o oráculo de US-10.
5. **Emissor de instrumentação.** Q5 respondida: reescrita de uma cópia do
   fonte, guiada pelas posições dos cursores já catalogados; teste com
   *golden* do arquivo instrumentado, mais um teste que compila o resultado.
6. **Cliente e UI.** `SourceFileViewer` ganha, quando o arquivo aberto contém
   função selecionada, um marcador clicável por entidade capturável na linha
   dela — mesma mecânica de linha clicável que US-5 já introduziu para chamadas
   (`calls`/`onCallSelected`), inclusive a mesma simplificação de resolução por
   linha e não por coluna.

---

#### US-6.3 — Estratégia de geração de execuções: execução real (fase A) e entradas sintéticas (fase B)

**Status:** planejado — **decisão tomada** (respostas 2 e 4): as duas vias, em
fases, com a real primeiro. Fase A implementável hoje; fase B bloqueada por
ferramenta ausente · **Depende de:** US-6.1, US-6.2

##### Objetivo do usuário

Que existam execuções da função selecionada para a instrumentação de US-6.2
gravar: na fase A, rodando o próprio programa do projeto sobre entradas reais;
na fase B, com entradas sintéticas geradas por KLEE sobre o código isolado.

##### Observações e decisões em aberto

- **Resolvido: as duas vias, nesta ordem.** A via real vem primeiro porque não
  depende de nada que falte no ambiente, e porque tira US-6 do caminho crítico
  de US-10 — mesmo argumento que `conversao-guiada-por-exemplos.md` §3 já fazia
  com o oráculo escrito à mão. A via KLEE não é descartada: ela substitui a
  origem das entradas sem mudar o que é gravado.
- **Resolvido, e é o achado mais consequente desta revisão: não há suíte de
  testes com que contar** (resposta 4 — o Verovio não tem). A via real precisa
  então de uma fonte de execuções própria, e a que existe é o **executável que
  o projeto já produz**. Daí o conceito novo de **perfil de execução**:

  ```
  RunProfile {
    id, nome,
    target,            // alvo executável do CMake
    args: Vec<String>, // ex.: ["partitura.mei", "-o", "saida.svg"]
    inputs: Vec<PathBuf>,
    working_dir,       // sempre dentro de <projeto>/characterization/
    expected_exit_code,
    timeout,
  }
  ```

  Uma suíte de testes, quando o projeto tiver uma, é apenas mais um perfil
  (`target = "meus_testes"`), não um caminho de código separado. Foi o que
  permitiu acomodar as duas realidades sem duplicar a implementação.
- **Descoberta dos alvos executáveis.** `compile_commands.json` lista unidades
  de compilação, não alvos — não dá para saber por ele o que vira executável. A
  fonte correta é a **CMake File API** (`codemodel-v2`): escrever um arquivo de
  consulta em `<build>/.cmake/api/v1/query/` antes de configurar e ler o
  `codemodel` resultante, que traz cada alvo com seu tipo (`EXECUTABLE`,
  `STATIC_LIBRARY`, …) e o caminho do artefato. Isso é uma extensão de
  `ingest.rs`, onde o `cmake` já é invocado, e vale a pena fazer na mesma
  configuração em vez de uma segunda.
- **Resolvido (Q6, parte i): o conceito de perfil de execução está confirmado.**
  O usuário fornece o alvo executável, os argumentos e os arquivos de entrada; o
  produto não tenta adivinhar execuções sozinho. A descoberta automática cobre
  apenas quais *alvos* existem (CMake File API, item acima) — qual usar e com
  que entradas é decisão do usuário.
- **Requisito adicional (Q6): criação de perfis em lote sobre um conjunto de
  arquivos.** Um perfil por arquivo escrito à mão não escala: o caso real é
  "caracterize este binário sobre cada um dos N arquivos do diretório X". O
  modelo precisa então de um **molde de perfil** — alvo, forma dos argumentos,
  `working_dir`, `expected_exit_code` e `timeout` fixos — que, aplicado a um
  conjunto de arquivos de entrada, gera N `RunProfile`s, um por arquivo. Duas
  consequências de projeto:
  - Os argumentos deixam de ser uma lista literal e passam a admitir um
    *placeholder* para o arquivo corrente (ex.: `["{input}", "-o",
    "{input_stem}.svg"]`), senão não há como variar a entrada mantendo o
    resto do comando. Sem isso, o lote só serviria para programas que leem de
    `stdin`.
  - Cada perfil gerado continua sendo um `RunProfile` de primeira classe,
    persistido individualmente, editável e removível — o lote é uma forma de
    *criar* perfis, não um tipo novo de perfil. Isso mantém US-6.3 e US-6.5 com
    um único conceito de execução, e é o que permite dizer "esta função foi
    alcançada por 7 dos 200 perfis" sem tratar lote como caso especial.
  A relação com a cobertura (`llvm-cov`, item abaixo) é direta: um lote grande é
  justamente o mecanismo pelo qual o usuário aumenta a cobertura de
  caracterização sem KLEE.
- **Resolvido (Q6, parte ii): os arquivos de entrada são copiados para dentro do
  diretório do projeto.** Cópia é *o* comportamento, não uma opção entre duas —
  não existe modalidade de perfil que referencie caminhos arbitrários da máquina
  do usuário, e portanto não há perfil "não reproduzível" a sustentar nem a
  sinalizar em US-6.5. As entradas vivem sob `<projeto>/characterization/`, o
  que mantém a execução reproduzível e preserva a regra "nada fora do diretório
  do projeto" — regra que pesa mais aqui do que em qualquer outro passo, já que
  este é o passo que executa código arbitrário vindo do input. Sob o Flatpak
  isso também é o que evita atrito: o portal de arquivos é acionado uma vez, na
  criação do perfil ou do lote, e nunca mais a cada execução. **Custo aceito:**
  um lote sobre um diretório de N arquivos copia os N; o diretório do projeto
  cresce com o tamanho do corpus de entrada.
- **Resolvido (Q7, parte i): o produto passa a compilar o projeto de entrada, e
  compila só os alvos necessários ao perfil escolhido.** Até US-5 o CMake era
  apenas *configurado*; ninguém nunca chamou `cmake --build`. A fase A exige
  compilar — e não uma vez só: a árvore instrumentada de US-6.2 é uma árvore de
  fontes distinta, que precisa ser recompilada a cada mudança nas marcações de
  captura do usuário. A compilação é feita via `cmake --build --target <alvo>`,
  com o conjunto mínimo de alvos **computado** a partir do grafo de dependências
  entre alvos que a CMake File API já fornece (item acima), nunca adivinhado.
  Duas consequências: paga-se menos que o pior caso a cada caracterização, e um
  alvo quebrado que o perfil não usa deixa de bloquear o trabalho.
  **Direção futura, fora da v1:** permitir que o usuário configure todos os
  alvos para teste/conversão diretamente pelo Syntax Bridge — isto é, o
  conjunto de alvos deixa de ser derivado apenas do perfil e passa a ser
  também uma escolha explícita do usuário, com a UI correspondente. O modelo
  de dados deve ser escrito de forma a acomodar isso (o conjunto de alvos a
  construir é um dado da caracterização, não uma constante derivada do
  `target` do perfil), mesmo enquanto a única forma de preenchê-lo for a
  derivação automática.
- **Uma compilação por alvo, não por perfil.** Com a criação de perfis em lote
  (Q6), N perfis sobre o mesmo alvo compartilham **uma** compilação e reusam o
  binário nas N execuções. Registrado explicitamente porque a implementação
  ingênua — compilar por perfil — multiplicaria por N o custo mais caro do
  passo.
- **Resolvido (Q7, parte ii): a compilação roda dentro do mecanismo de job.**
  Reaproveita o job de US-1/US-4 (progresso + cancelamento), o que o torna a
  **quarta** instância dele: a requisição devolve `job_id` na hora, `JobPhase`
  ganha uma fase de construção, o progresso é relatado enquanto o
  `cmake --build` roda, e `DELETE /projects/jobs/{id}` interrompe a compilação
  em andamento. Compilar sincronamente dentro da requisição HTTP está
  descartado: é exatamente o erro já cometido e corrigido em US-1, que produziu
  o travamento relatado no Verovio 5.7.0.
- **Cobertura sem KLEE, e isso resolve o critério 2.** Compilar a árvore
  instrumentada com `-fprofile-instr-generate -fcoverage-mapping` e processar o
  `.profraw` com `llvm-profdata merge` + `llvm-cov export` dá cobertura de
  linha e de região por função. `llvm-profdata` e `llvm-cov` vêm da extensão
  `llvm21`, já no manifesto — nenhuma ferramenta nova. A cobertura entra no
  registro da execução (US-6.5) e é o que permite dizer ao usuário "esta função
  foi caracterizada em 40% dos seus ramos", em vez de deixá-lo supor 100%.
- **Uma função selecionada que nenhum perfil alcança é um resultado, não um
  erro.** Precisa aparecer como "0 execuções, não caracterizada" — é a
  informação que diz ao usuário que falta um perfil, e é o que impede US-10 de
  reportar cobertura de prova inflada.
- **Fase B (KLEE) permanece descrita em US-6.4**, junto com o isolamento de que
  depende. Papel de cada ferramenta: KLEE descobre entradas que cobrem os
  ramos, GoogleTest materializa e executa os casos.

##### Critérios de aceitação (testáveis)

1. Para uma função pura simples do fixture alcançada por um perfil de execução,
   rodar o perfil produz ao menos um registro de comportamento.
2. A cobertura efetivamente exercitada é medida (`llvm-cov`) e reportada por
   função — nunca presumida.
3. Rodar o mesmo perfil duas vezes sobre a mesma entrada produz o mesmo
   conjunto de registros.
4. Uma função selecionada que nenhum perfil alcança aparece com zero execuções
   e marcada como não caracterizada.
5. Um perfil cujo executável termina com código diferente do esperado é
   reportado como falha do perfil, com a saída padrão e de erro preservadas, e
   não invalida os registros já coletados por outros perfis.
6. Um perfil que estoura o timeout é interrompido, e isso é registrado como
   tal (ver US-6.5).
7. *(Fase B)* Os casos gerados por KLEE cobrem todos os ramos da função
   isolada, ou a lacuna é reportada.

##### Condições de testabilidade

- **Fase A é testável no ambiente de destino hoje:** `cmake`, `clang++`,
  `llvm-profdata` e `llvm-cov` estão todos no Flatpak.
- O fixture precisa ser um projeto CMake que **produz um executável** e aceita
  um argumento de entrada — o `sample-cmake-project` atual provavelmente
  precisa ganhar um alvo executável, ou surge um fixture irmão. Escolher
  entradas que exercitem apenas parte dos ramos, de propósito, para que o
  critério 2 tenha o que reportar como parcial.
- O teste de escala (rodar um perfil sobre o Verovio) segue o precedente já
  estabelecido: `#[ignore]`d por padrão, como
  `verovio_5_7_0_import_diagnosis.rs` e o teste de cancelamento de US-4.
- Determinismo (critério 3) exige que o perfil não dependa de relógio, de rede
  nem de caminho absoluto da máquina — mais um argumento para copiar as
  entradas para dentro do projeto (Q6).
- Fase B depende de US-6.4 e de KLEE no ambiente (ver Q10).

##### Roteiro de implementação (para um agente) — fase A

1. **Descoberta de alvos.** Estender `crates/server/src/ingest.rs` para emitir a
   consulta `codemodel-v2` da CMake File API antes de configurar e ler os
   alvos resultantes. Teste primeiro em `crates/server/tests/project_ingest.rs`:
   `ingest_lists_executable_targets_of_the_cmake_project`. Persistir em tabela
   `build_targets` e devolver junto de `CreatedProject`/`LoadedProject`.
2. **Perfis de execução (CRUD).** Tabela `run_profiles`, rotas
   `GET`/`PUT`/`DELETE /projects/characterization/run-profiles`, testes de rota
   sem toolchain. UI: painel `run_profiles_view.dart` com o alvo escolhido de
   uma lista (a de 1), argumentos e arquivos de entrada.
3. **Build instrumentado.** `crates/server/src/characterization/build.rs`:
   configura e compila a árvore instrumentada de US-6.2 em
   `<projeto>/characterization/build/`, com as flags de cobertura, usando
   `cmake --build --target <alvo>`. Roda dentro de um job
   (`JobPhase::BuildingInstrumented`), com `Cancellation` checado entre alvos
   e a saída do compilador em log — o precedente de "silêncio por minutos é
   indistinguível de travamento" já está pago em US-1 e não deve ser repetido.
4. **Execução e coleta.** `characterization/run.rs`: executa o perfil com
   `LLVM_PROFILE_FILE` apontando para dentro do projeto, timeout do perfil,
   captura de `stdout`/`stderr`, e recolhe os traces que o *runtime* de US-6.2
   escreveu. Grava um `characterization_run` e seus `behavior_traces` (US-6.5).
5. **Cobertura.** `characterization/coverage.rs`: `llvm-profdata merge` +
   `llvm-cov export --format=text`, reduzido a percentual por função e gravado
   no registro da execução. Teste com um fixture cuja entrada exercita
   deliberadamente só um dos dois ramos, afirmando que o relatório diz isso.
6. **Rota e UI.** `POST /projects/characterization/runs` (dispara, devolve
   `job_id`, reaproveitando `jobs.rs`), `GET /projects/characterization/runs`.
   Painel de caracterização mostrando, por função selecionada: número de
   execuções, cobertura e o último trace.

---

#### US-6.4 — Isolamento por função (*program slicing* e *stubs*)

**Status:** **adiado (fora da v1)** por Q10 — fase B apenas; não bloqueia o
primeiro incremento de US-6, e US-6 é dado por completo com a fase A ·
**Depende de:** US-6.3 (fase A entregue) e da reversão de Q10 para KLEE —
GoogleTest já foi incorporado ao manifesto Flatpak em 2026-08-13 (ver
AGENTS.md), mas KLEE segue fora, e é ele quem ainda bloqueia este sub-passo

##### Objetivo do usuário

Gerar código isolado por função — a função em questão mais as definições
mínimas necessárias para compilá-la e executá-la sozinha, cobrindo todos os
ramos.

##### Observações e decisões em aberto

- **Granularidade do isolamento.** O plano antigo falava em isolar *unidades
  de compilação* (criando mocks de tudo o que cada uma precisa); este passo
  fala em isolar *funções*. São coisas diferentes e provavelmente ambas
  necessárias: a unidade de compilação é o que compila, a função é o que se
  caracteriza. Decidir se a função isolada é compilada dentro de uma TU
  sintética própria (com mocks) ou extraída da TU original.
- **Isolar "com as definições mínimas" é *program slicing*.** O mecanismo já
  existe parcialmente: o fecho transitivo sai do grafo `type_dependencies` de
  US-3 somado ao grafo de chamadas de US-5. O que falta é a política de
  corte — onde parar e substituir por *stub*.
- **Papel de cada ferramenta:** KLEE para descobrir entradas que cobrem todos
  os ramos; GoogleTest para materializar e executar os casos.
- **Funções não puras são a maioria e o caso difícil:** I/O, estado global,
  alocação, ponteiros recebidos, tempo, aleatoriedade, concorrência. Isso pesa
  sobretudo sobre esta via (KLEE precisa sintetizar entradas em torno desses
  efeitos); na via real de US-6.3, um efeito colateral é só mais um dado
  observado, não um obstáculo à busca de entradas. Definir quais categorias
  são caracterizáveis por KLEE na v1 e quais são explicitamente marcadas como
  "não caracterizada, requer decisão humana".

##### Critérios de aceitação (testáveis)

1. Para uma função pura simples do fixture, o código isolado gerado compila
   sozinho.
2. Os casos gerados cobrem todos os ramos dessa função, e a cobertura é
   medida, não presumida.
3. Uma função com dependência não satisfazível é marcada como não
   caracterizada, com motivo, sem interromper as demais.

##### Condições de testabilidade

- O fixture precisa incluir, de propósito, uma função pura, uma função com
  efeito colateral em global, uma que recebe ponteiro, e uma que não termina.
- KLEE precisa estar disponível no ambiente Flatpak — hoje não está no
  manifesto (GoogleTest já está, desde a revisão de Q10 em 2026-08-13; ver
  AGENTS.md). Enquanto KLEE não estiver, este sub-passo não é testável no
  ambiente de destino (ver "Observações transversais → Ambiente de teste").

##### Roteiro de implementação (para um agente)

**Não comece por aqui — e, por ora, não comece de jeito nenhum.** Com a fase A
entregue, este sub-passo deixou de ser pré-requisito de qualquer coisa e passou
a ser uma melhoria de cobertura; Q10 o declarou fora da v1 para a parte que
depende de KLEE. Sem KLEE no ambiente, o trabalho não tem como ser provado por
teste, e a regra de ouro do `AGENTS.md` não admite começar assim. O roteiro
abaixo fica registrado para o dia em que a parte de KLEE de Q10 for revista com
dado de cobertura em mãos.

**Resolvido (Q10): KLEE e GoogleTest ficavam adiados — nenhum entrava no
manifesto Flatpak por ora.** Revisto em 2026-08-13, parcialmente: **GoogleTest
foi incorporado** ao manifesto (`build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`,
módulo `googletest`, construído via CMake a partir do release v1.18.0 — ver
AGENTS.md), por decisão explícita do usuário. **KLEE continua fora** — era a
alternativa descartada mais pesada das duas: arrastaria LLVM próprio, um SMT
solver e uma biblioteca C substituta, de longe o módulo mais pesado que o
manifesto teria. A via sintética (fase B) continua fora da v1 e US-6 continua
dado por completo com a fase A, porque GoogleTest sozinho não a destrava —
falta quem descubra as entradas que cobrem os ramos, e esse é o papel do KLEE.

O adiamento de KLEE não é definitivo: a decisão de incluí-lo volta à mesa
quando houver **dado** — a fase A roda sobre o Verovio, `llvm-cov` reporta
quanta cobertura os perfis de execução reais alcançam, e a diferença para 100%
é o tamanho exato do problema que KLEE resolveria. Decidir antes disso seria
decidir sem medida. Dois fatos empurram a favor do adiamento: a criação de
perfis em lote (Q6) é o mecanismo pelo qual o usuário aumenta cobertura sem
KLEE, e US-6 inteiro é opcional para o usuário (ver "US-6 é opcional de ponta a
ponta"), o que tira a fase B de qualquer caminho crítico.

Se e quando a resposta sobre KLEE virar "entra", a ordem é:

1. **Manifesto primeiro**, com um teste de disponibilidade no padrão de
   `crates/server/tests/toolchain_availability.rs`. A metade de GoogleTest já
   está feita (módulo `googletest` no manifesto,
   `googletest_compiles_and_runs_a_small_test_suite`); falta só a de KLEE. Sem
   isso, nada abaixo é testável no ambiente de destino.
2. **Fatiamento (*slicing*)**, em `crates/server/src/characterization/slice.rs`:
   fecho transitivo sobre `type_dependencies` (US-3) mais `call_edges` (US-5) a
   partir da função alvo — a mesma travessia de US-6.1, reaproveitada, com
   política de corte declarada. Teste com asserção sobre o conjunto de
   declarações incluídas, sem compilar nada.
3. **Emissão da TU sintética** com *stubs* nos cortes; critério 1 (compila
   sozinha) provado chamando `clang++` sobre a saída.
4. **KLEE + GoogleTest** para produzir e executar os casos, gravando no **mesmo**
   esquema de US-6.5 que a fase A já usa. Se for preciso mudar o esquema aqui,
   pare: o formato da fase A foi mal projetado e é ele que precisa de conserto.

---

#### US-6.5 — Persistência do comportamento observado, limites de execução e segurança

**Status:** planejado · **Depende de:** US-6.3 (e de US-6.4 quando a via
escolhida exigir isolamento)

##### Objetivo do usuário

Confiar que o comportamento observado — por qualquer via de US-6.3 — fica
gravado de forma recuperável e determinística, que execuções que não terminam
são interrompidas e reportadas, e que rodar código arbitrário do projeto de
entrada não representa risco para a máquina do usuário.

##### Observações e decisões em aberto

- **O resultado deste sub-passo é o oráculo de US-10.** Sem essa frase, o
  passo parece documentação; com ela, fica claro que é a base da prova de
  equivalência da conversão. O esquema de gravação dos traces precisa ser
  projetado para essa comparação, não só para exibição — ver formato do
  registro em US-6.2.
- **Não determinismo e limites de execução:** timeout por função, o que fazer
  quando o KLEE não converge (a maioria dos casos reais, só relevante se
  US-6.3 mantiver essa via), teto de caminhos explorados, e como isso é
  reportado sem parecer falha.
- **Segurança — posição, agora escrita.** Este passo compila e executa código
  arbitrário vindo do input do usuário. A posição é: **tudo acontece dentro do
  *sandbox* do Flatpak, e tudo escreve exclusivamente sob
  `<projeto>/characterization/`.** O Flatpak não tem rede nem gerenciador de
  pacotes, o que aqui deixa de ser a limitação que atrapalha US-1 e vira a
  garantia que sustenta US-6. O que o produto **não** promete: proteção contra
  um input malicioso que consuma CPU ou disco dentro do sandbox — o timeout e
  o teto de disco mitigam, não eliminam. Isso precisa estar dito ao usuário,
  não presumido.
- **Esquema proposto** (duas tabelas, para que a execução e o comportamento
  observado não se misturem):
  - `characterization_runs (id, source, run_profile_id, started_at,
    finished_at, exit_code, status, coverage_json, log_path)` — `source` em
    `real`|`synthetic`, `status` em `completed`|`timeout`|`failed`|`cancelled`.
  - `behavior_traces (run_id, function_usr, invocation_seq, entry_json,
    exit_json, schema_version, truncated)`.
  O que varia entre execuções (horário, duração, código de saída) fica na
  primeira tabela; o que precisa ser idêntico entre execuções (critério 2) fica
  na segunda. Sem essa separação, o critério 2 nunca passa.
- **O `invocation_seq` é necessário e sutil:** uma função chamada N vezes numa
  execução produz N traces, e a ordem deles é parte do comportamento. Manter a
  ordem de invocação é o que permite a US-10 comparar sequências de chamadas, e
  não apenas conjuntos.
- **Relação com `oracle/cases.json`.** `conversao-guiada-por-exemplos.md` §5.3
  propõe um formato de caso de comportamento escrito à mão para a escada de
  exemplos, e diz que ele é "o embrião do registro de comportamento de US-6".
  Isso deve ser levado a sério nos dois sentidos: **quem implementar primeiro
  define o formato, e o outro se adapta**. Se a escada chegar antes,
  `behavior_traces.entry_json`/`exit_json` adotam o formato dela; se US-6.2
  chegar antes, a escada regrava seus casos. Dois formatos concorrentes para a
  mesma coisa seria o pior resultado possível, porque US-10 consome os dois.

##### Critérios de aceitação (testáveis)

1. O comportamento observado (entradas, saída, efeitos) é gravado no banco e
   pode ser recuperado.
2. Executar a caracterização duas vezes sobre o mesmo código produz o mesmo
   registro (comparando `behavior_traces`, não `characterization_runs`).
3. Uma função/execução que entra em laço infinito é interrompida pelo timeout
   e registrada como `status = "timeout"`, com os traces já coletados até ali
   preservados.
4. Nenhuma etapa deste passo escreve fora do diretório do projeto.
5. Uma execução cancelada pelo usuário (`DELETE /projects/jobs/{id}`) para e
   fica registrada como `cancelled`, sem corromper registros anteriores.
6. Traces de funções que saíram do conjunto efetivo de US-6.1 continuam
   legíveis, marcados como órfãos.

##### Condições de testabilidade

- Determinismo é pré-requisito: sem entradas fixadas e sem controle de tempo e
  aleatoriedade, o critério 2 é impossível e o sub-passo inteiro deixa de ser
  testável. A política de serialização de US-6.2 (sem endereços, sem
  timestamps no payload, ordenação estável) é o que torna isso alcançável.
- Precisa haver um modo de execução com limites (tempo, memória, teto de disco)
  configuráveis pelo teste, senão a suíte fica lenta ou intermitente.
- O critério 3 exige um fixture com laço infinito deliberado, e o teste precisa
  de timeout curto (~1s) para não travar a suíte.
- O critério 4 é testável por asserção sobre o sistema de arquivos: gravar a
  árvore antes e depois e comparar tudo fora de `<projeto>/`.

##### Roteiro de implementação (para um agente)

1. **Tabelas e round-trip primeiro** (é o item 3 do roteiro de US-6.2, feito
   aqui): `characterization_runs` e `behavior_traces` em `project_store.rs`,
   com testes inline. Antes do emissor de instrumentação, para que ele já
   escreva no formato final.
2. **Rotas de leitura:** `GET /projects/characterization/runs` e
   `GET /projects/characterization/traces?function_usr=…`, testadas populando o
   banco direto, sem executar nada — mesmo padrão de
   `usages_route_returns_the_persisted_usages_for_a_type`.
3. **Limites de execução:** timeout por perfil e teto de disco em
   `characterization/run.rs`, ambos configuráveis, com o fixture de laço
   infinito provando o critério 3.
4. **Cancelamento:** reaproveitar `progress::Cancellation`, checado entre
   perfis e, no build, entre alvos. Nada novo — é o mesmo `AtomicBool` de US-4.
5. **Teste de confinamento** (critério 4): um teste que roda uma caracterização
   completa e afirma que nenhum arquivo fora de `<projeto>/` mudou.
6. **UI:** o painel de caracterização mostra execuções, status, cobertura e
   traces; um trace com `truncated` visível como tal, nunca silenciosamente.

---

## US-7 — Mapeamento de tipos C++ → Dart

**Status:** parcial — solver por regras implementado e testado contra o
corpus de `docs/mapping-solver-cases.md` (22 casos, `mapping-solver-fixtures/`
na raiz), além da fatia mínima do E03
(`docs/plans/primeiro-corte-e01-e03.md` PR5). `crates/server/src/mapping.rs`:
`MappingOption { id, label, description, consequences: Vec<Consequence> }`,
`MappingDecision { type_usr, option_id, decided_at }`, `ProjectFacts`
(catálogo de tipos + usos + funções + grafo de chamadas, de onde o solver
lê de verdade agora — não só a assinatura preparada para isso), e cinco
pontos de entrada: `options_for` (tipo), `overload_options_for` (grupo de
sobrecarga), `template_options_for` (monomorfização local vs. decisão
global), `signature_options_for` (ponteiro/inteiro de largura
fixa/`float`/`setjmp`/`goto`/thread-mutex, por assinatura ou varredura
textual do corpo) e `string_usage_conflict` (`std::string` texto vs. binário,
projeto inteiro). Critério 1 satisfeito e testado: `struct`/`class` sem
herança múltipla devolve exatamente uma opção. Critério 2 satisfeito e
testado: herança múltipla devolve uma combinação classe+mixins com
consequências, e resolve o conflito de diamante (duas bases declarando o
mesmo método) sobrescrevendo explicitamente em vez de confiar na ordem de
`with`. Critério 3 satisfeito para o caso testado (B01: uma opção que
mutaria outro tipo por referência não-const é documentada como restrição, não
oferecida como se não houvesse consequência). Critério 5/Q9 satisfeito e
testado: qualquer tipo sem mapeamento direto devolve uma opção de código
ponte, nunca lista vazia. Critério 6 satisfeito e testado: toda opção que
teria efeito em outro tipo carrega `Consequence` estruturado citando esse
tipo. Persistência: tabela `type_mappings (type_usr PRIMARY KEY, option_id,
decided_at)` em `project_store.rs`, com `set_type_mapping` (upsert, não
`replace_*`/`delete`-then-`insert` como as demais tabelas — decisões são
dado do usuário, não catálogo derivado, e não podem ser apagadas quando
outro catálogo é reextraído) e `list_type_mappings`; critério 4 (reabrir
preserva a decisão) e critério 7 (round-trip) provados por teste inline.
`decisions.toml` (mesmo subconjunto flat de TOML que `example.toml` usa) lido
e aplicado ao banco sem passar pela UI, cruzado contra `options_for` de
verdade (não um id escrito à mão) em
`e03_decisions_toml_applies_to_the_database_without_going_through_the_ui`.
**O E07 é a primeira consulta de verdade a `overload_options_for` pela própria
geração** (`function_catalog::apply_overload_renames`, não mais só os testes
unitários do solver): agrupa declarações por `(owning_class_usr, name)`,
consulta o solver uma vez por grupo, e — quando a decisão é
`"renomear-por-tipo"`/`"renomear-const-nao-const"` — renomeia a
declaração e todo call site que a referencia, por `usr`, numa segunda
passada sobre o módulo inteiro (`examples/E07-sobrecarga-e-parametros-default/NOTES.md`).
`"parametro-opcional"` (sobrecargas que só diferem em aridade) é uma decisão
que o solver já devolve mas a geração ainda não age sobre — agir exigiria
fundir duas entradas de IR num único `Function`/`Method` com parâmetro
opcional à direita, um tipo de mudança diferente de renomear, e nenhum
fixture força isso ainda. **Falta:** o solver de viabilidade global de
verdade (Q9 completo — E09 é quem dimensiona isso; o que existe hoje é regras
heurísticas sobre fatos já extraídos, não uma satisfação de restrições real —
ver as limitações conhecidas registradas em `docs/mapping-solver-cases.md`,
casos B06 e C03), as rotas `GET`/`PUT /projects/mappings`, a UI, agir sobre
`"parametro-opcional"`, e — importante — **`transpile::transpile` ainda não
consulta `type_mappings` nem `options_for`** (o mapeamento de *tipo*, distinto
do de *sobrecarga* que o E07 já consome) **ao gerar Dart**: `Ponto` é sempre
emitido como classe diretamente, sem checar se existe decisão gravada. Isso é
honesto para E01–E07 porque todo tipo evolvido até aqui só tem uma opção
possível (nada a decidir), mas significa que o critério "tipo sem decisão
produz falha explícita" não está de fato conectado ponta a ponta para
*tipos* — só valeria a pena resolver quando um tipo tiver mais de uma opção
de verdade. **Catálogo de ponteiros:** `crates/server/src/pointer_catalog.rs`
extrai, com `libclang`, todo ponteiro bruto declarado no projeto (parâmetro,
campo, variável local, retorno de função — `PointerDeclarationKind`), sua
forma (`PointerShape`: `Scalar`/`DoublePointer`/`FunctionPointer`) e, quando o
apontado é um tipo que `type_catalog` já conhece, o `usr` desse tipo —
persistido em `pointer_declarations` e servido por `GET /projects/pointers`,
mesmo nível de US-2/US-3. Ainda **não é consumido** por
`mapping::pointer_options_for`/`possible_pointee_types`, que continuam sobre a
varredura textual (`signature.contains('*')`) e a enumeração por hierarquia de
classes (CHA) descritas abaixo — ver
`docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`, Parte 2 · **Depende de:**
US-4, US-5

### Objetivo do usuário

Para cada tipo, ver o mapeamento óbvio em Dart quando ele existir (classe ou
mixin) e, quando não existir, ver as opções e suas consequências ("se isto for
mixin, então aquilo muda"). O Syntax Bridge filtra as opções ao máximo, para
apresentar apenas as que resultam em Dart compilável — se escolher X para a
classe A inviabiliza a classe B, essa opção não deveria ser oferecida. Quando
não houver nenhuma escolha direta possível, apresentar opções de código *ponte*
que permitam prosseguir na conversão.

### Observações e decisões em aberto

- **Herança múltipla é um item de uma lista maior**, e a lista é o coração do
  produto. Precisa estar escrita, com posição para cada caso:
  - herança múltipla → classe + mixins + interfaces;
  - ponteiros, aritmética de ponteiros e referências;
  - semântica de valor vs. referência (cópia profunda, construtor de cópia,
    atribuição, *move*);
  - RAII e destrutores — Dart não tem destrutor determinístico;
  - templates → genéricos de Dart ou monomorfização por instância;
  - sobrecarga de função → parâmetros nomeados/opcionais ou renomeação;
  - sobrecarga de operadores (Dart cobre um subconjunto);
  - `const`-correctness;
  - `union`;
  - inteiros de largura fixa, `unsigned` e overflow (o `int` de Dart é 64 bits
    na VM e difere na web);
  - ponto flutuante e precisão;
  - `char*` e `std::string` → `String` ou `Uint8List`;
  - contêineres da STL → coleções de Dart;
  - exceções, `goto`, `setjmp`;
  - preprocessador e compilação condicional;
  - concorrência (threads → isolates, com semântica de memória distinta);
  - `dart:ffi` como escape final quando não houver conversão viável.
- **Resolvido (Q9): viabilidade global é resolvida de fato, não apenas
  alertada.** Filtrar opções por viabilidade global é um problema de satisfação
  de restrições, não uma verificação local: a escolha em A propaga por todo o
  grafo de tipos. A decisão foi **contra** a recomendação original (alertar
  depois da escolha) e a favor de resolver, por um motivo de propósito de
  produto, não de custo: *resolver o mapeamento de tipos entre linguagens é o
  objetivo principal do Syntax Bridge*. Um produto que aceita a escolha e depois
  informa que ela quebrou o projeto empurra ao usuário exatamente o trabalho que
  ele veio delegar. Consequências, todas assumidas:
  - **Só opções válidas são apresentadas.** O critério 3 vale na sua redação
    forte ("não é oferecida"), não na alternativa ("é oferecida marcada como
    conflitante"). A redação alternativa do critério 3 fica revogada.
  - **O conjunto de opções nunca é vazio**, porque quando nenhum mapeamento
    direto é viável o produto **gera código ponte** que torna a conversão
    possível (ver o item seguinte, que esta resposta também fecha em parte).
    Isso é o que mantém o problema tratável: sempre existe uma atribuição
    satisfazível, então o solver busca as combinações válidas em vez de ter que
    provar insatisfazibilidade global e travar o usuário sem saída.
  - **Custo aceito:** o produto ganha um componente de satisfação de restrições
    sobre o grafo de tipos de US-3 — dimensão comparável à do resto do servidor,
    e o item mais caro de US-7. Cada opção precisa ter suas restrições
    expressas como dado verificável por máquina (é o que o `Consequence`
    estruturado do roteiro já exige), senão não há o que resolver.
  - **Evidência ainda vem do E09.** Herança múltipla é onde o conflito real
    aparece, e continua sendo o degrau que dimensiona o solver. O que muda é
    que ele nasce solver desde o começo, ainda que trivial nos degraus
    iniciais, em vez de nascer validador e ser trocado depois.
- **As decisões do usuário são o ativo mais valioso do projeto** e precisam ser
  persistidas com identidade estável de tipo (ver US-3) para sobreviver a US-12.
- **Ordem de decisão importa:** decidir tipos folha antes de tipos que dependem
  deles reduz retrabalho; o grafo de US-3 já dá essa ordem.
- **Decisão registrada: o solver de ponteiros evolui de CHA para TFA/DFA.**
  `possible_pointee_types` hoje é class-hierarchy analysis (CHA): sobe a
  hierarquia do tipo apontado e enumera toda subclasse alcançável, sound mas
  superestimado sempre que a hierarquia é maior que os usos reais, porque não
  olha se algum código de fato atribui aquela subclasse a aquele ponteiro. A
  direção decidida é aproximar de uma análise de fluxo de tipos (type-flow
  analysis, no espírito de RTA/points-to), usando o grafo caller/callee que
  `function_catalog::CallEdge`/`CallResolution` já expõe (`is_dynamic_dispatch`
  para despacho virtual, `Unresolved` para chamada por ponteiro de função) como
  substrato interprocedural, e os sites de atribuição do catálogo de ponteiros
  acima como substrato intraprocedural. Plano completo, com o corpus de teste a
  construir (`mapping-solver-fixtures/`, categoria B) e a regra de nunca
  under-approximate: `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`.
- **Código ponte: papel decidido (Q9), forma ainda em aberto.** O papel ficou
  definido pela resposta de Q9: código ponte é o que garante que o conjunto de
  opções de um tipo nunca seja vazio — quando nenhum mapeamento direto é viável,
  o produto gera o código intermediário que torna a conversão possível, em vez
  de declarar o tipo não convertível. Segue em aberto **qual é a forma** desse
  código: adaptador gerado, classe manual com TODO, ou `dart:ffi`. Sem essa
  definição o último item do passo não é implementável, e agora ela também é
  pré-requisito do solver — uma opção que o solver possa sempre oferecer
  precisa ser uma opção que o emissor de US-8 saiba materializar.

### Critérios de aceitação (testáveis)

1. Uma classe C++ sem herança múltipla recebe um mapeamento direto para classe
   Dart, sem apresentar alternativas.
2. Uma classe com herança múltipla recebe pelo menos uma combinação
   classe+mixin viável, com as consequências descritas.
3. Uma opção que tornaria outro tipo do projeto não convertível **não é
   oferecida** — redação forte, conforme a resposta de Q9. A redação
   alternativa que este critério admitia ("ou é oferecida marcada como
   conflitante") está revogada.
4. Escolher uma opção e reabrir o projeto preserva a escolha.
5. Um tipo sem mapeamento direto possível recebe ao menos uma opção de código
   ponte — e, por Q9, a lista de opções de qualquer tipo é sempre não vazia.
6. Cada opção apresentada declara explicitamente o que muda nos tipos
   dependentes.

### Condições de testabilidade

- O fixture precisa conter um caso de herança múltipla com conflito real, e um
  par de tipos em que a escolha de um restringe o outro — sem isso os critérios
  2 e 3 não têm como ser exercitados.
- A viabilidade precisa ser verificável por máquina: ou por regra declarada, ou
  por geração e compilação de um trecho Dart mínimo. A segunda torna o teste
  lento porém honesto.
- Decisões precisam ser expressáveis como dado (não como interação de UI) para
  que os testes de servidor não dependam do cliente.

### Roteiro de implementação (para um agente)

**Não implemente este passo em largura.** A lista de 18 construções acima é um
*checklist a consumir*, não um lote. A ordem de consumo é a escada de
`conversao-guiada-por-exemplos.md`: o E03 força o caso trivial (uma opção só),
o E07 força a primeira escolha real (sobrecarga), o E09 força a viabilidade
global (herança múltipla). Cada degrau fecha um punhado de itens do checklist e
nada mais.

1. **Modelo de decisão, antes de qualquer regra.** Em
   `crates/server/src/mapping.rs`: `MappingOption { id, rótulo, descrição,
   consequences: Vec<Consequence> }` e
   `MappingDecision { type_usr, option_id, decided_at }`. `Consequence` carrega
   o `usr` do tipo afetado e o que muda nele — é o critério 6, e ele precisa
   ser dado estruturado, não texto livre, senão o critério 3 não tem como ser
   verificado por máquina.
2. **Persistência:** tabela `type_mappings (type_usr TEXT PRIMARY KEY, option_id,
   decided_at)`, chaveada pelo `usr` de US-3 — é exatamente aqui que a
   identidade estável se paga, e é o que faz o critério 1 de US-12 passar.
3. **Regras, uma por vez, cada uma com teste próprio.** `mapping::options_for(
   declaration, catalog, decisions)` devolve as opções de um tipo — já
   filtradas por viabilidade global (Q9), e por isso dependentes das decisões
   já tomadas, não só da declaração. A primeira versão devolve uma opção única
   para classe sem herança múltipla (critério 1) e uma opção de código ponte
   com motivo para todo o resto — a lista nunca é vazia, e silêncio é proibido.
4. **Solver de viabilidade** (Q9, decidido a favor de resolver): as restrições
   de cada opção são dado estruturado sobre o grafo de US-3, e
   `mapping::feasible_options(type_usr, catalog, decisions)` devolve só o que
   mantém o grafo inteiro satisfazível. Função pura sobre catálogo e decisões,
   testável sem banco. Nasce trivial nos degraus iniciais (E03: uma opção só) e
   é dimensionado pelo E09, onde herança múltipla produz o primeiro conflito
   real — mas nasce solver, não validador *a posteriori*, porque trocar um pelo
   outro depois muda o contrato de `options_for` e a UI que o consome.
5. **Rotas:** `GET /projects/mappings` (tipos, opções viáveis, decisão atual) e
   `PUT /projects/mappings`. Testes de rota populando o banco direto.
6. **Ordem de decisão na UI:** ordenar os tipos pela ordem topológica do grafo
   `type_dependencies` de US-3, com os tipos folha primeiro. Não é enfeite —
   é o que evita retrabalho, e o grafo já existe.
7. **Decisões como dado, sem UI:** um formato de arquivo (`decisions.toml`, na
   proposta da escada) aplicável ao banco antes de transpilar. É o que permite
   testar US-8 sem cliente, e é condição de testabilidade já registrada acima.

---

## US-8 — Geração do código Dart

**Status:** parcial — fatia do E01–E12 (`docs/plans/primeiro-corte-e01-e03.md`,
PRs 1–5, e `docs/plans/conversao-guiada-por-exemplos.md` para o E04–E12): IR em
`crates/server/src/ir/` (`Module`, `Function`, `Record` (com
`destructor: Option<Vec<Stmt>>`, E12 — nunca emitido como membro, só
consumido pela síntese de RAII), `Field`, `Param`
(com `default_value: Option<Expr>`, E07),
`Method`, `Constructor`, `BaseClass`,
`Type::{Int, Bool, Double, Void, Record, Str, List, Unsupported}`,
`Stmt::{Return, VarDecl, Assign, FieldAssign, If, While, For, ExprStmt,
Throw, TryCatch, TryFinally, Unsupported}` (as três últimas, E12 —
`TryFinally` nunca lowered de um cursor C++, só sintetizado),
`Expr::{IntLiteral, DoubleLiteral, BoolLiteral, StringLiteral, Ref, Binary,
Unary, Call, FieldAccess, RecordConstruct, ConstructorCall, This, Index,
StringByteLength, Unsupported}`, `BinaryOp` com aritmética/comparação/`And`,
`UnaryOp::Neg`, tudo com `Origin`), lowering em
`crates/server/src/lower/cpp.rs` como extensão do passe de `function_catalog`
(critério do roteiro item 1: sem quarta passada `libclang`) — inclusive
`lower_record`/`lower_method`/`lower_constructor`, chamados do mesmo
`visit_cursor` para `StructDecl`/`ClassDecl` e para definições de
método/construtor (inline ou fora da classe) sem cortar a recursão —, emissor
determinístico em `crates/server/src/emit/dart.rs`, orquestração em
`crates/server/src/transpile.rs` e rota síncrona `POST /projects/transpile`.
Critérios 1–4 satisfeitos e testados para funções livres com aritmética,
comparação, `if`/`else`, `while`, `for`, recursão, negação unária, `struct`
POD com semântica de valor, classe com métodos, `this` implícito, visibilidade
(`private`/`protected` → `_` no nome Dart), campo estático e construtor
múltiplo (nomeação por ordinal: o primeiro vira o construtor sem nome da
classe, os demais viram `ClassName.ctorN` — Dart não tem sobrecarga de
construtor por assinatura), um **adaptador de biblioteca padrão**:
`std::string` → `String`, `std::vector<T>` → `List<T>`, reconhecidos por
`lower::cpp::stdlib_template_name` (nome do template primário + namespace
`std`, não a soletração do tipo) em vez de `lower_record` — `.size()` de
string vira ponte UTF-8 (`Expr::StringByteLength`), `.size()` de vetor vira
`.length` direto, `operator[]` de vetor vira `Expr::Index`, `operator+`/`==`
de string viram `Expr::Binary` (Dart já sobrecarrega os três nativamente) —,
e agora **herança simples e `virtual`**: `Record.base_class` (`extends`),
`Method.body: Option<Vec<Stmt>>` (`None` = método virtual puro, vira
assinatura sem corpo em Dart), `Method.is_override` (`clang_getOverriddenCursors`,
não casamento de nome) → `abstract class`/`@override` (`tests/lower_cpp.rs`,
`tests/emit_dart.rs`, `tests/transpile.rs`, `tests/transpile_route.rs`,
`tests/conversion_examples.rs`); critério 5
(silêncio proibido) também testado — inclusive o caso em que um
`Stmt::Unsupported` em qualquer profundidade (corpo, `if`, `while`, `for`)
precisa derrubar a função inteira, não só a si mesmo, senão statements
seguintes referenciam nomes nunca declarados em Dart (achado registrado em
`examples/E01-funcao-aritmetica/NOTES.md`). A armadilha do E02 — `int / int`
trunca em C++ e precisa virar `~/` em Dart, não `/` — está resolvida e testada
(`examples/E02-controle-de-fluxo/NOTES.md`), decidida pelo tipo do próprio nó
`Binary`, não por inspecionar os operandos. A armadilha do E03 — C++ copia um
`struct` passado por valor, Dart passa a referência — está resolvida e testada
(`examples/E03-struct-pod/NOTES.md`): `lower::cpp::
collect_params_with_clone_prelude` insere um autoclone (`p = Ponto(p.x, p.y);`)
como primeiro statement do corpo para todo parâmetro `Record` **por valor**
(checado contra o tipo *cru* do parâmetro, não o `ir::Type` já desembrulhado —
o E06 precisou reforçar essa distinção quando um parâmetro por referência a um
`Record` do próprio usuário apareceu pela primeira vez), regra geral, não por
fixture. A armadilha do E04 — `this` implícito não aparece como filho
visitável de um `MemberRefExpr` no `libclang` (só no `ast-dump` interno do
Clang) — está resolvida e testada (`examples/E04-classe-com-encapsulamento/NOTES.md`):
`lower::cpp::member_ref_receiver` trata zero filhos como `Expr::This`
diretamente. A armadilha do E05 — `std::string` conta bytes UTF-8,
`String.length` conta *code units* UTF-16, e os dois discordam fora de ASCII —
está resolvida e testada com ponte de código, não declarada como divergência
conhecida (`examples/E05-biblioteca-padrao/NOTES.md`, caso `"ação"`: 6 bytes
vs. 4 code units, `utf8.encode(x).length` bate com o C++ nos dois). A
armadilha do E06 — destrutor virtual não tem equivalente em Dart — está
resolvida por omissão deliberada (`examples/E06-heranca-simples/NOTES.md`):
`function_catalog` já distingue `Destructor` de `Method` desde o US-5, e
simplesmente nunca despacha um destrutor para `lower_method`/`Record::methods`
— nenhum corpo de destrutor deste corpus tem lógica de limpeza real, então
"não emitir nada" é honesto (RAII de verdade é E12). A armadilha do E07 —
renomear uma sobrecarga obriga a reescrever todo call site — está resolvida
e testada (`examples/E07-sobrecarga-e-parametros-default/NOTES.md`):
`function_catalog::apply_overload_renames` roda depois que todo
`Function`/`Method` já foi lowered com o nome C++ original, agrupa
declarações por `(owning_class_usr, name)`, consulta
`mapping::overload_options_for` uma vez por grupo — a primeira consulta real
desse solver pela própria geração, não só pelos testes do solver — e quando a
decisão exige renomear, monta um mapa `usr → novo nome`
(`function_catalog::dart_overload_name`: nome original + tipo Dart de cada
parâmetro capitalizado) e varre o módulo inteiro trocando `Expr::Call.callee_name`
por `callee_usr`, nunca por nome. Um valor default de parâmetro
(`int passo = 1`) é mapeamento direto — parâmetro opcional posicional do
Dart (`[int passo = 1]`) — sem passar pelo solver, já que não é sobrecarga. A
armadilha do E08 — especialização explícita e SFINAE: recusar, não adivinhar
— está resolvida sem precisar de um caminho de recusa dedicado
(`examples/E08-templates/NOTES.md`): toda instanciação de template de função
(implícita ou especialização explícita) é lowered a partir do próprio cursor
já resolvido (`referenced`), que o `libclang` entrega com tipos concretos
substituídos e, no caso de uma especialização explícita, com o corpo
realmente escrito para aquele tipo — nunca o corpo do template primário
reinterpretado com `T` trocado mecanicamente. `lower::cpp::
monomorphized_template_name` nomeia cada instanciação de forma determinística
(`dobro` + tipo Dart concreto de cada parâmetro → `dobroInt`/`dobroDouble`/
`dobroString`), a mesma função usada tanto para renomear a declaração
(`function_catalog::record_call`, que agora também sintetiza o
`ir::Function` de uma instantiação implícita nunca visitada de outra forma)
quanto todo call site — computada independentemente nos dois lugares a
partir do mesmo cursor, nunca por referência cruzada. `overload_type_suffix`
é compartilhado com o esquema de renomeação de sobrecarga do E07
(`function_catalog::dart_overload_name`), uma implementação só. A armadilha
do E09 — estado em mixin, ordem de linearização — não precisou de código
novo além da própria representação (`examples/E09-heranca-multipla/NOTES.md`):
`Record.mixins` (populado quando uma classe tem mais de um `CXXBaseSpecifier`
— `base_class`, do E06, continua cobrindo exatamente um) faz toda base virar
mixin Dart (`with A, B`, nunca `extends`), e um `Record` referenciado como
mixin em qualquer lugar do módulo (`emit::dart::emit_module` varre o `Module`
inteiro antes de emitir qualquer arquivo) é emitido como `mixin`, não
`class` — sem construtor algum (Dart proíbe) e com todo campo já
valor-zero na própria declaração. Acesso a campo/método herdado de um
mixin (`pato.altitude`, `pato.subir()`) e a resolução de qual `mover()`
"vence" não precisaram de nada novo: já operam no nível do cursor que
*declara* o membro, não de qual `Record` o possui no Dart gerado. A
armadilha do E10 — talvez a resposta certa seja recusar — já funcionava de
graça para ponteiro cru (nenhum caso de `lower_type` trata `CXType_Pointer`,
então cai no catch-all `Type::Unsupported` desde o E01) e revelou um bug
real para `union` (`examples/E10-ponteiros-union-out-params/NOTES.md`):
`union` compartilha `CXType_Record` com `struct`/`class` no Clang, então sem
tratamento próprio virava `Type::Record{usr, name}` apontando para uma
classe que `function_catalog::visit_cursor` nunca despacha para
`lower_record` (só `StructDecl`/`ClassDecl`) — uma referência pendurada,
`dart analyze` acusando `undefined_class`, pega no primeiro fixture que
teve um `union` de verdade. Corrigido recusando explicitamente
(`Type::Unsupported`) assim que `lower_type` reconhece
`CXCursor_UnionDecl`, antes de resolver usr/nome. A armadilha do E11 —
header incluído em N TUs duplica declaração — já não acontecia na IR
usada para geração (a dedup por `usr` na junção entre workers, existente
desde o E01, já cobria isso de graça); a lacuna real era a ausência de
qualquer `import` entre arquivos Dart gerados, nunca exercitada até um
fixture com mais de um `.cpp` existir
(`examples/E11-multi-tu/NOTES.md`). `emit::dart::emit_module` monta
`usr_to_stem` (todo `Record`/`Function` de nível superior → arquivo que o
declara) e `emit_file` caminha a própria árvore (`collect_referenced_usrs_in_*`,
mesmo padrão do `rename_calls_in_*` do E07, mas coletando em vez de
renomear) para decidir quais `import '<outro>.dart';` imprimir — chamada
de método entre arquivos fica de fora (só `usr` de `Record`/`Function` é
mapeado), documentado como lacuna sabida, não silenciosa. A armadilha do
E12 — RAII não tem construto Dart equivalente — está resolvida por síntese,
não por tradução direta (`examples/E12-excecoes-raii/NOTES.md`):
`function_catalog::apply_raii_scope_guards` roda no mesmo ponto de
pós-processamento que `apply_overload_renames` (E07), procura o primeiro
`VarDecl` de nível superior de uma função livre cujo tipo tem
`Record.destructor` com corpo real, e envolve tudo depois dele num
`ir::Stmt::TryFinally` cujo `finally` é o próprio corpo do destrutor (já
lowered), com `Expr::This` substituído pela referência ao local
(`replace_this_with_ref_in_stmts`/`_stmt`/`_expr`, terceiro caminhador
mecânico da escada, mesmo padrão do E07/E11). `throw`/`try`/`catch` em si
mapeiam quase direto para o próprio `throw`/`try`/`on T catch` do Dart. Duas
armadilhas colaterais apareceram só ao montar o fixture: um `DeclRefExpr`
para campo estático referenciado de *fora* da classe (função livre lendo
`Guarda::contadorAberto`) precisava de qualificação (`Guarda.contadorAberto`)
que `lower_expr` nunca tinha produzido antes — corrigido com
`qualified_static_member_name`, aplicada incondicionalmente (dentro e fora
da classe, já que não há "classe atual" para decidir por contexto), o que
também reescreveu (corretamente) a saída do E04 para os próprios acessos
internos da classe ao seu campo estático; e um guard cujo destrutor só toca
estado estático (nunca `this`) deixa a variável local sem nenhuma referência
no Dart emitido, `unused_local_variable` do `dart analyze` — corrigido
verificando, depois da substituição `This`→`Ref`, se o nome do guard aparece
em algum dos dois corpos; se não aparece, a declaração vira uma expressão
solta (só o construtor, pelo efeito colateral) em vez de um `VarDecl`
nomeado. Múltiplos `catch` no mesmo `try` e `throw`/`catch` sem operando
(`throw;`, `catch (...)`) recusam explicitamente (`Unsupported`), e a
passagem de RAII só cobre o primeiro guard de nível superior de uma função
livre — nem guard aninhado em `if`/`while`/`for`, nem guard dentro de
método/construtor, nem uma segunda local RAII na mesma função (que exigiria
`try`/`finally` aninhados) — nenhum fixture força nenhum dos dois ainda,
lacunas documentadas, não silenciosas. **O E13** ("degrau de realidade" — uma
fatia de `include/vrv/fraction.h`/`src/fraction.cpp` do Verovio 6.2.0,
extraída e não escrita para o produto, `examples/E13-fatia-real-verovio/
NOTES.md`) **fica `passa`**, depois de uma primeira rodada em que ficou
`esperado-falhar` de propósito: seis lacunas reais
apareceram, nenhuma vista por nenhum fixture sintético em doze degraus —
inicialização por construtor direto (`Tipo var(args);`, em vez da forma por
cópia que E01–E12 sempre usaram), `static_cast<T>` explícito (só a promoção
*implícita* `int`→`double` do E02 é lowered), atribuição composta (`/=`, e
por extensão `+=`/`-=`/`*=` — todo fixture anterior só escreveu a forma
expandida), chamada de operador de usuário (`a == b`) de *fora* da própria
classe (`lower_method_call` espera um `MemberRefExpr` como receptor, forma
que uma chamada de operador não produz — a própria *definição* do operador
sempre traduziu bem, o buraco é só no call site), um método estático e um
de instância com o mesmo nome (válido em C++ por assinatura, proibido em
Dart — `conflicting_static_and_instance`), e a assinatura fixa que Dart
exige de `operator==` (`Object`, não o tipo do próprio usuário —
`invalid_override`, mesmo com o método corretamente lowered e emitido).
Cada um foi corrigido depois, um a um, com os doze degraus anteriores
continuamente verdes a cada passo — e corrigi-los revelou mais três lacunas
que eles mascaravam: chamada de método estático de fora da classe (nunca
tentada antes: nenhum fixture chamava um método estático de fora de sua
classe antes do E13), parâmetro de saída via referência não-`const`
(`int&`, o idioma de "out param" que `examples/E10-ponteiros-union-out-
params/NOTES.md` tinha identificado e decidido não construir — resolvido
com uma ponte genuína via `ir::Type::Tuple`/`Expr::Tuple`/`Stmt::TupleAssign`,
records nativos do Dart 3: a função vira `(T, T) f(...)` e o call site vira
`(a, b) = Classe.f(a, b);`), e `std::gcd` (sem equivalente top-level em
Dart, mas `int` já tem o método nativo `.gcd()`). Ver "Resolução" em
`examples/E13-fatia-real-verovio/NOTES.md` para os nove achados e a
correção de cada um. Os doze degraus anteriores continuam verdes depois de
todas as correções. No
cliente, `client/flutter/lib/src/ui/dart_output_view.dart`
(painel "Dart Output" em `server_status_page.dart`, acionado pelo botão
"Transpile" da barra de título) já mostra o Dart gerado ao lado do arquivo C++
aberto, casando pelo stem do nome de arquivo (`matchingDartPath`) — cobre a
parte do objetivo do usuário de "obter o código Dart correspondente" enquanto
não existe navegação arquivo-a-arquivo automática. **Falta para "pronto":**
`transpile::transpile` ainda não consulta `mapping::options_for`/`type_mappings`
para *tipos* (distinto de `overload_options_for`, já consultado desde o E07 —
US-7 está pronto o bastante para o mapeamento de tipo em E01–E12 porque cada
tipo evolvido até aqui só tem uma opção — nada a decidir), agir sobre a
decisão `"parametro-opcional"` do solver de sobrecarga (fundir duas
sobrecargas de aridade diferente num único `Function`/`Method` — ver
`examples/E07-sobrecarga-e-parametros-default/NOTES.md`), consultar
`mapping::template_options_for`/o lado de herança múltipla de
`mapping::options_for` (os dois solvers já existem e já decidem — inclusive
detectando conflito de diamante para herança múltipla — mas a geração
segue estratégias fixas que não checam o resultado, porque nenhum fixture
ainda expõe uma escolha real entre alternativas viáveis; só passa a
importar a partir de quando um cenário multi-TU (E11) ou de conflito winner
fizer diferença — ver `examples/E08-templates/NOTES.md` e
`examples/E09-heranca-multipla/NOTES.md`), template de método (só função
livre por enquanto), `std::string`/`std::vector` por valor (todo parâmetro
do E05 é `const T&`, de propósito — ver
`examples/E05-biblioteca-padrao/NOTES.md`), ponte real para ponteiro/`union`
(`dart:ffi` — E10 recusa honestamente em vez de construir a ponte; o outro
idioma que E10 tinha deixado sem ponte, o de out param via referência
não-`const`, foi resolvido pelo E13 — ver
`examples/E10-ponteiros-union-out-params/NOTES.md` e
`examples/E13-fatia-real-verovio/NOTES.md`), `import` entre
arquivos para chamada de método (só `Record`/`Function` de nível superior
são mapeados — ver `examples/E11-multi-tu/NOTES.md`), nome de `library`
Dart derivado de `namespace` C++ (capturado desde o US-3/US-5, nunca usado
na geração), guard RAII aninhado em `if`/`while`/`for`, dentro de
método/construtor, ou mais de um guard na mesma função (exigiria
`try`/`finally` aninhados — ver `examples/E12-excecoes-raii/NOTES.md`),
múltiplos `catch`/`throw` ou `catch` sem operando, e qualquer coisa além
do que E01–E13 cobrem (`break`/`continue`, `i++`/`--i`, construtor de
subclasse chamando `super(...)` explicitamente) · **Depende de:** US-7

### Objetivo do usuário

Obter o código Dart correspondente ao projeto C++, a partir dos mapeamentos
decididos.

### Observações e decisões em aberto

- **O modelo intermediário deixou de ser exigência do `AGENTS.md`** (foi
  retirado da lista de fronteiras) — mas o argumento técnico que o motivava
  continua de pé: gerar Dart diretamente de cursores do `libclang` amarra o
  emissor a C++ e faz a extensibilidade por adaptador custar uma reescrita.
  **Q8 respondida** (ver observações transversais): a retirada significa que a
  IR deixou de ser fronteira obrigatória e passou a ser detalhe interno de
  US-8 — `crates/server/src/ir/`, crescendo degrau a degrau da escada de
  exemplos, não projetada em largura de antemão.
- Ordem de geração sai da ordem topológica do grafo de tipos de US-3; ciclos
  precisam de política própria.
- Definir o mapeamento de estrutura de projeto: arquivos, bibliotecas, `part`,
  diretórios, e o `pubspec.yaml` gerado.
- Definir o que acontece com o que não foi decidido em US-7: gera com TODO,
  omite, ou bloqueia a geração.
- Rastreabilidade: cada trecho Dart gerado deve apontar para sua origem C++, o
  que é o que permite a navegação e o diagnóstico em US-9 e US-10.
- **US-8.1 — Preferências de estilo do código gerado, opcional** (aspas,
  largura de linha, vírgula final, `final`/`const`, preset de lint do pacote
  exportado): plano completo em `docs/plans/estilo-de-codigo-gerado.md`. Não
  bloqueia US-8 nem depende de mais nenhuma ferramenta — reusa `dart
  format`/`dart fix`, já no manifesto Flatpak.

### Critérios de aceitação (testáveis)

1. Para o fixture com todas as decisões tomadas, a geração produz um pacote
   Dart com estrutura válida.
2. Cada tipo do catálogo com decisão tomada tem uma declaração correspondente no
   Dart gerado.
3. Gerar duas vezes com as mesmas decisões produz saída idêntica byte a byte.
4. Cada declaração gerada é rastreável até arquivo e linha de origem em C++.
5. Um tipo sem decisão produz falha explícita ou marcação visível, nunca
   silêncio.

### Condições de testabilidade

- Geração determinística (critério 3) é o que torna todo o resto testável por
  comparação de saída; exige ordenação estável e nada de iteração sobre
  estruturas não ordenadas.
- Precisa existir um conjunto de decisões de US-7 gravável diretamente pelo
  teste, sem passar pela UI.

### Roteiro de implementação (para um agente)

Este é o passo em que a escada de `conversao-guiada-por-exemplos.md` §7 já tem
o roteiro detalhado — siga-o, e trate o que está abaixo como o resumo que
amarra aquele plano a este documento. A ordem é: infra do corpus, E01 fino,
oráculo, UI, e então um degrau por vez.

1. **Nada de quarta passada `libclang`.** A extração para IR é uma extensão de
   `function_catalog::extract_function_catalog_cancellable`, que já é a única
   passada que parseia corpos. Isso é a mesma regra que US-6.2 segue, e pela
   mesma razão: a seção "Escala" já registra três passadas, e uma quarta é
   inaceitável.
2. **`Unsupported` é um nó de primeira classe da IR, desde a primeira versão.**
   Com origem (arquivo, linha) e motivo. O emissor o transforma em falha
   explícita ou `TODO` visível. É o critério 5, e é a regra "silêncio é
   proibido" no ponto onde ela mais importa: Dart que compila e está errado é o
   único resultado inaceitável.
3. **Determinismo desde o primeiro commit** (critério 3): ordenação estável em
   toda coleção emitida, e nenhuma iteração sobre `HashMap`. Retrofitar
   determinismo depois é caro; nascer com ele é de graça.
4. **Rastreabilidade (critério 4)** também desde o primeiro commit: cada nó da
   IR carrega sua origem C++, e o emissor a propaga. Ela é pré-requisito de
   US-9 (critério 3) e de US-10 (critério 3) — os dois passos seguintes
   dependem dela, então adicioná-la depois significa refazer os dois.
5. **A rota nasce síncrona** (`POST /projects/transpile`); quando o custo
   aparecer, o `jobs.rs` já resolve progresso e cancelamento, e é reaproveitado
   como US-4 e US-5 fizeram. Não invente um segundo mecanismo.

---

## US-9 — Validação estática do Dart gerado

**Status:** parcial — critérios 1 e 2 já vinham satisfeitos (ver histórico
abaixo); critério 3 agora também, com a rota e o painel próprios.
`transpile::transpile` (`crates/server/src/transpile.rs`) já encana todo
`.dart` emitido pelo `dart format --output=show` (lendo de stdin) antes de
devolver o pacote — não por replicar à mão a heurística de quebra de linha do
`dart_style` (tentativa inicial que quebrou em
`dart format --set-exit-if-changed`, ver
`examples/E01-funcao-aritmetica/NOTES.md`), mas invocando o formatador real.
`tests/transpile.rs` roda `dart analyze`/`dart format --set-exit-if-changed`
de verdade sobre o pacote escrito em disco, inclusive para um caso com nó
`Unsupported`. **Resolvido: critério 3**, a tradução de diagnóstico para
arquivo/linha C++ de origem —
`crates/server/src/validate/dart.rs` (`DartDiagnostic { severity, message,
dart_file, dart_line, origin: Option<ir::Origin> }`, `analyze_package`)
roda `dart analyze --format=json` sobre o pacote já escrito e traduz cada
achado de volta à declaração C++ de origem via `locate_origin`. **A
granularidade é a declaração de topo (função livre, ou `class`/`mixin`, ou
`enum`) inteira, não a instrução exata** — decisão deliberada, não uma
lacuna silenciosa: um mapa linha-a-linha pré-formatação não sobrevive ao
reflow de `dart format` (que roda sobre o arquivo inteiro e pode mover
qualquer linha), então `locate_origin` localiza cada declaração pelo próprio
texto já formatado — o mesmo que `dart analyze` de fato consultou — via um
cursor sequencial que nunca casa a declaração errada (busca na mesma ordem
de emissão de `emit::dart::emit_file`: enums, depois records, depois
funções). Quando nenhuma declaração é localizada (uma linha de `import`, ou
o helper sintético `_syntaxBridgeUnsupported`), `origin` é `None` — nunca um
palpite. Provado por 6 testes unitários em `validate/dart.rs` (função pura,
sem tocar o toolchain) e por `tests/validate_dart.rs`, que roda o `dart
analyze` real sobre um `TranspiledPackage` deliberadamente quebrado — a
mesma receita que o roteiro abaixo pedia ("um pacote Dart com erro
deliberado, cuja origem o teste conhece"), sem depender de nenhuma
construção C++ que o produto ainda mistraduza (não existe uma, hoje —
`emit::dart` nunca emite Dart inválido). **Resolvido: rota e UI.**
`POST /projects/validate` (`project_service::validate_project`,
`server.rs`) transpila o projeto (mesmo caminho de
`POST /projects/transpile`, via `build_transpiled_package` compartilhado) e
devolve `{"diagnostics": [DartDiagnostic]}`; provado por
`tests/validate_route.rs`, inclusive o caso "IR persistida reutilizada" nos
mesmos moldes de `transpile_route.rs`. Painel "Validation"
(`client/flutter/lib/src/ui/diagnostics_view.dart`) ao lado do "Dart
Output", aberto pelo botão "Validate" da toolbar; clicar num diagnóstico com
origem resolvida navega ao C++ (mesma mecânica de `_selectUsage`/
`_selectCallTarget`), testado em `diagnostics_view_test.dart` e
ponta-a-ponta em `app_test.dart`. Capturas em
`docs/screenshots/README.md` (`us9-validation-diagnostics`,
`us9-validation-clean`). **Decisão registrada (roteiro item 3): avisos
informam, erros não bloqueiam** — a rota devolve todo diagnóstico
(`ERROR`/`WARNING`/`INFO`), sem filtrar nem impedir nenhuma ação posterior;
a UI só usa a severidade para peso visual (erros primeiro, em vermelho),
mesmo espírito de "nunca bloquear por prova incompleta" que `AGENTS.md` já
aplica a US-6. **Falta:** granularidade por método/instrução dentro de uma
classe (hoje todo o corpo de uma `class` resolve para a origem da própria
declaração da classe); `dart format --set-exit-if-changed` não é chamado por
este módulo (comportamento já coberto por `transpile::emit_package`, que
formata antes de qualquer validação — nada aqui verifica de novo, então uma
regressão nesse contrato não apareceria como um `DartDiagnostic`); e a
suíte deste passo ainda não rodou dentro do Flatpak (rodou no host, com o
mesmo Dart SDK 3.12.2 do manifesto — ver "Condições de testabilidade")
· **Depende de:** US-8

### Objetivo do usuário

Saber que o Dart gerado é válido antes de tentar executá-lo, com os erros
apontando de volta para o C++ de origem.

### Observações e decisões em aberto

- Ferramentas: `dart analyze` e `dart format` sobre o pacote gerado. O Dart SDK
  **já está** no manifesto Flatpak (módulo `dart-sdk`, versão 3.12.2 com
  `sha256` fixado, em `build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json`),
  então este passo é testável no ambiente de destino desde já.
- Diagnósticos do analisador precisam ser traduzidos para a origem C++ pela
  rastreabilidade de US-8; um erro Dart sem essa correlação é inútil para o
  usuário.
- Definir se avisos do analisador bloqueiam ou apenas informam.

### Critérios de aceitação (testáveis)

1. **Satisfeito.** O pacote gerado para o fixture passa em `dart analyze` sem
   erros — `tests/transpile.rs`.
2. **Satisfeito.** O código gerado já está no formato de `dart format`
   (formatar não produz diferença) — `tests/transpile.rs`.
3. **Satisfeito.** Um erro do analisador é apresentado com o arquivo e a
   linha C++ de origem, na granularidade descrita acima —
   `validate::dart::analyze_package`/`locate_origin`, provado por
   `tests/validate_dart.rs` (subprocesso `dart analyze` real) e
   `tests/validate_route.rs` (rota HTTP completa).

### Condições de testabilidade

- Dart SDK disponível no ambiente de teste — e, para valer, dentro do Flatpak.
  Satisfeita: o manifesto instala o SDK em `/app/lib/dart-sdk` e expõe
  `/app/bin/dart`.
- Versão do SDK fixada, senão a saída do analisador varia entre máquinas.
  Satisfeita: 3.12.2, por URL de release com `sha256`.

### Roteiro de implementação (para um agente) — concluído

Os cinco itens abaixo foram todos implementados numa única sessão; mantidos
aqui, marcados, como registro de como o passo foi feito (a "receita padrão"
da introdução não cobria rota+UI de validação, então este roteiro próprio
seguiu valendo até o fim).

1. ✅ **`crates/server/src/validate/dart.rs`:** invoca `dart analyze
   --format=json` sobre o pacote já escrito (`dart format` não é chamado de
   novo aqui — ver "Falta" acima) e traduz a saída para `DartDiagnostic
   { severity, message, dart_file, dart_line, origin: Option<ir::Origin> }`.
2. ✅ **Tradução para a origem C++** (critério 3) — `locate_origin`, testada
   primeiro por um pacote Dart com erro deliberado
   (`tests/validate_dart.rs`), exatamente como pedido aqui.
3. ✅ **Avisos informam, erros não bloqueiam** — registrado no doc comment de
   `DiagnosticsView` (`client/flutter/lib/src/ui/diagnostics_view.dart`): a
   rota devolve todo diagnóstico sem filtrar, a UI só usa a severidade para
   peso visual.
4. ✅ **Rota** `POST /projects/validate` (`project_service::validate_project`)
   e painel "Validation" na UI, clique levando ao C++ de origem via
   `SourceFileViewer`/mesma mecânica de US-4/US-5 (`_selectDiagnostic` em
   `server_status_page.dart`).
5. ⚠️ **Pendente:** a suíte deste passo rodou no host (mesmo Dart SDK 3.12.2
   do manifesto), não ainda dentro do Flatpak via `just test`.

---

## US-10 — Prova de equivalência comportamental

**Status:** parcial — a fatia do E01–E12, pela fonte
"casos escritos à mão da escada" (`docs/plans/primeiro-corte-e01-e03.md`
PR3/PR4/PR5, e `docs/plans/conversao-guiada-por-exemplos.md` para o
E04–E12), com suporte a
argumento agregado desde o E03 (`{"x": 3.0, "y": 4.0}` em `oracle/cases.json`,
resolvido contra `ir::Record` re-extraído no próprio harness, emitido como
`Ponto{3.0, 4.0}` para C++ e `Ponto(3.0, 4.0)` para Dart — ordem de campo
vem do `ir::Record`, não da ordem das chaves no JSON). O E04 não exigiu
nenhuma mudança no harness do oráculo em si — construtor com argumento
`double`, método com `this` implícito e campo estático já cabem no mesmo
formato de caso de função livre com argumentos escalares. O E05 exigiu duas
formas de literal novas — string (JSON string → `"..."` escapado igual nos
dois lados) e vetor (JSON array → `std::vector<int>{...}`/`[...]`, só
elemento `int`, sem despacho por tipo de elemento) — mais o `espera` de uma
função poder ser uma string comparada por igualdade de texto (o próprio
armadilha de `"ação"` do E05 é um caso do oráculo, não um teste separado). O
E06 também não exigiu nenhuma mudança no harness — despacho dinâmico
(`Cachorro`/`Gato` via `Animal&`) e classe abstrata só aparecem *dentro* dos
corpos das funções livres testadas (`testarCachorro`/`testarGato`), nunca
como argumento/retorno do próprio caso, então o mesmo formato de string já
bastava. O E07, pela mesma razão, também não — sobrecarga renomeada e
parâmetro default só aparecem dentro dos corpos testados, nunca na própria
assinatura de caso do oráculo. O E08, mais uma vez — instanciação de
template e monomorfização só aparecem dentro dos corpos testados
(`testarDobroInt`/`testarDobroDouble`/`testarDobroString`), nunca na própria
assinatura de caso. O E09, pela mesma razão de novo — herança múltipla,
mixin e estado herdado só aparecem dentro dos corpos testados
(`testarAltitude`/`testarProfundidade`/`testarMovimento`). O E10 também não —
a única função com caso de oráculo (`somarSemPonteiro`) nunca usa ponteiro
nem `union`; as funções que usam ficam de fora de `oracle/cases.json` de
propósito, porque `Unsupported` lança em tempo de execução (correto — é
esse o critério — mas incompatível com o oráculo chamar e comparar saída).
O E11 exigiu multi-TU real pela primeira vez (`collect_oracle_sources` já
compila todos os `.cpp`/`.hpp` de `src/` juntos, e `run_dart_oracle` já
importava todo `lib/*.dart` gerado no seu próprio driver — os dois já
cobriam multi-arquivo de graça); a lacuna que faltava era os arquivos
gerados se importarem *entre si*, que é código de produção
(`emit::dart`), não do harness. Implementado dentro do
próprio harness (`crates/server/tests/conversion_examples.rs`, não em
`crates/server/src/`, já que o oráculo por enquanto só serve o corpus de
exemplos): `run_cpp_oracle` compila e executa um `main` C++ sintético contra
as flags reais do `compile_commands.json`, `run_dart_oracle` roda o
equivalente sobre o Dart **transpilado** com `dart run`,
`compare_oracle_outputs` reduz os dois a forma canônica e compara — a fonte
da verdade é o C++ executado, `espera` em `oracle/cases.json` é só
conferência de sanidade (critério testado:
`espera` errado no exemplo aponta o erro para o exemplo, não para o
produto). Critério 3 (teste de mutação) satisfeito por
`mutation_test_a_sabotaged_dart_emitter_is_caught_by_the_oracle`: como
recompilar o emissor mutado dentro do próprio processo de teste não é
prático, o teste alimenta `compare_oracle_outputs` — a função de produção
real, não uma cópia — com C++ real e um pacote Dart sabotado à mão
(byte-a-byte o que `emit_binary_op` produziria com `+` trocado por `-`), e
afirma que a mensagem de erro carrega origem e os dois valores. Uma
divergência conhecida e declarada (overflow de `int` de 32 vs. 64 bits, E01)
é tratada como informação (`divergencia_conhecida` em `oracle/cases.json`),
não como falha — mas o harness falha se os dois lados um dia passarem a
concordar, para a premissa não apodrecer em silêncio. O E12 também não
exigiu nenhuma mudança no harness — `throw`/`try`/`catch` capturado
(`testarExcecaoCapturada`) e o guard RAII fechando escopo
(`testarGuardaFechaAoSair`, comparando o contador estático antes/depois de
`usarGuarda()` sair) são só mais duas funções livres com retorno `int`,
mesmo formato de caso já usado desde o E01; o comportamento novo (exceção
lançada e capturada, destrutor rodando na saída do `try`/`finally`
sintetizado) é exercitado pela própria execução real dos dois lados, não
por nenhuma extensão do formato do oráculo. O E13, na sua primeira rodada,
nunca chegou a exercitar o oráculo — o harness parava no `dart analyze`
(critério 2 de US-9) antes de rodar `oracle/cases.json`, e a fatia real de
`Fraction` falhava ali por seis razões catalogadas em
`examples/E13-fatia-real-verovio/NOTES.md`. Depois de corrigidas (ver
"Resolução" naquele arquivo), o oráculo passou a rodar de verdade — e um
décimo problema apareceu só então, não de tradução mas do próprio fixture:
`uso.cpp` era o único arquivo do corpus sem um `.hpp` declarando seus
`testarX()`, então o driver C++ do oráculo (que só `#include`s headers, não
`.cpp`s, para descobrir as assinaturas testadas) não compilava — corrigido
adicionando `uso.hpp`, mesma convenção de todo outro degrau. Os seis casos
de `oracle/cases.json` (mesmo formato de função livre com retorno escalar
desde o E01) rodam e concordam entre C++ real e Dart transpilado. **Falta:**
critério 1
como declarado (associar caso↔função do catálogo, não só nome), critério 4
(relatório de fração de funções provadas), a tabela de regras de
equivalência por tipo de `crates/server/src/equivalence.rs` (hoje a
comparação é só igualdade textual canônica de inteiros/booleanos/`double`
— `double` com `std::setprecision(15)` do lado C++ para reduzir, sem
eliminar, o descompasso com o `toString()` de Dart; comparação por bits
fica para quando `equivalence.rs` existir — o que E01+E02 precisam, nada
além), e qualquer ligação com US-6/`behavior_traces` (a
fonte "casos escritos à mão" é deliberadamente a única usada até aqui) ·
**Depende de:** US-8, mais uma fonte de oráculo — a fase A de US-6 ou os
casos escritos à mão da escada de exemplos. **Deixou de depender de US-6
inteiro**, e portanto de KLEE, quando a rodada 1 decidiu que US-6 tem duas
fases (ver "As duas fases" em US-6)

### Objetivo do usuário

Confiar que o Dart gerado se comporta como o C++ original — não porque compila,
mas porque foi testado contra o comportamento observado do original.

### Observações e decisões em aberto

- **Este passo é o que dá sentido a US-6.** O comportamento gravado lá é o
  oráculo: para cada caso caracterizado em C++, gerar o teste Dart equivalente,
  executá-lo e comparar.
- Comparar valores entre linguagens exige uma noção definida de equivalência:
  inteiros com largura diferente, ponto flutuante, ordem de coleções, strings e
  codificação, e ponteiros (que simplesmente não têm correspondente).
- Definir o destino das divergências: bloqueiam a exportação, viram relatório,
  ou alimentam de volta as decisões de US-7.
- Funções não caracterizadas em US-6 permanecem não provadas — a cobertura da
  prova precisa ser visível ao usuário, e não implícita.
- **US-6 é opcional, e o caso "nenhuma caracterização" é normal.** Como o
  usuário pode escolher não rodar US-6 (ver "US-6 é opcional de ponta a ponta"),
  o estado de zero oráculo não é erro nem projeto incompleto: US-10 simplesmente
  não tem o que provar e **reporta cobertura de prova zero**, explicitamente. O
  que ele não pode fazer é bloquear US-11 nem sugerir que a conversão está
  verificada.

### Critérios de aceitação (testáveis)

1. Para cada caso caracterizado em US-6, existe um teste Dart correspondente.
2. Os testes gerados passam para as funções puras do fixture.
3. Uma divergência introduzida de propósito no gerador é detectada e reportada
   com origem e valores esperado/obtido.
4. O relatório informa a fração de funções efetivamente provadas.

### Condições de testabilidade

- Requer US-6 determinístico; sem isso a comparação é ruído.
- Requer regras de equivalência escritas por tipo, senão cada divergência vira
  discussão caso a caso em vez de asserção.
- Precisa de um teste de mutação (critério 3) para provar que a suíte
  efetivamente detecta erro — uma suíte que só passa não prova nada.

### Roteiro de implementação (para um agente)

**Este passo não precisa esperar US-6.** O oráculo pode vir de duas fontes com
o mesmo formato: os casos escritos à mão da escada de exemplos
(`oracle/cases.json`) e os `behavior_traces` da fase A de US-6. Comece pelos
primeiros — é o que `conversao-guiada-por-exemplos.md` §11 item 3 propõe — e
troque a fonte depois, sem tocar no comparador.

1. **Regras de equivalência primeiro, como tabela declarada.**
   `crates/server/src/equivalence.rs`, com uma entrada por par de tipos: inteiro
   de 32 bits vs. `int` de 64, `double` vs. `double` (comparação por bits),
   `std::string` (bytes) vs. `String` (UTF-16), ordem de coleções, e ponteiro —
   que simplesmente **não tem correspondente** e precisa de veredito próprio
   ("não comparável", não "igual"). Cada regra nasce com teste unitário; nenhuma
   nasce como comparação genérica por igualdade estrutural, que esconderia
   exatamente as divergências que este passo existe para achar.
2. **Runner duplo:** executar o caso em C++ (compilado com as flags reais do
   `compile_commands.json`) e em Dart (`dart run` sobre o pacote gerado),
   reduzindo os dois a uma forma canônica antes de comparar. **A verdade é o
   C++ executado**, não o valor que alguém escreveu como esperado — o `espera`
   escrito à mão é conferência de sanidade.
3. **Teste de mutação junto do primeiro caso, não no fim** (critério 3): trocar
   `+` por `-` no emissor e exigir que o oráculo falhe, com origem e valores
   esperado/obtido. Uma suíte que passa e não falha quando sabotada não prova
   nada, e descobrir isso tarde custa o dobro.
4. **Relatório de cobertura de prova** (critério 4): fração de funções com pelo
   menos um caso comparado, com as não caracterizadas visíveis por nome. Na
   fase A de US-6 esse número vem baixo de propósito — é honesto, e é o que
   impede o usuário de confundir "compila" com "está provado".
5. **Destino das divergências:** relatório, não bloqueio, na primeira versão —
   uma divergência conhecida e declarada (o overflow de `int` do E01 é o
   exemplo canônico) é informação, não falha.

---

## US-11 — Exportação do projeto convertido

**Status:** planejado · **Depende de:** US-9 e, quando houver prova, US-10 —
mas **não** de US-6: como a caracterização é opcional para o usuário (ver "US-6
é opcional de ponta a ponta"), exportar um projeto sem prova de equivalência é
um caminho legítimo, desde que o relatório de exportação declare o que não foi
provado

### Objetivo do usuário

Levar embora o resultado: um pacote Dart utilizável fora do Syntax Bridge.

### Observações e decisões em aberto

- Definir o conteúdo do pacote: código, `pubspec.yaml`, testes gerados,
  relatório de conversão, itens pendentes.
- Definir se o relatório de o que *não* foi convertido acompanha a exportação —
  deveria: é a informação de que o usuário precisa para terminar o trabalho à
  mão.
- Exportar sob o sandbox do Flatpak exige portal de arquivos; o destino não é um
  caminho arbitrário.

### Critérios de aceitação (testáveis)

1. A exportação produz um diretório ou archive que compila com o Dart SDK fora
   do Syntax Bridge.
2. O pacote inclui os testes gerados em US-10 e eles passam.
3. O relatório lista todo item não convertido, com motivo.

### Condições de testabilidade

- O teste precisa validar o pacote exportado *fora* do diretório do projeto,
  para provar que ele não depende do ambiente de origem.

### Roteiro de implementação (para um agente)

1. **Montagem do pacote** em `crates/server/src/export.rs`: código gerado,
   `pubspec.yaml`, testes de US-10, `CONVERSION_REPORT.md` e a lista de
   pendências. Determinística, como US-8.
2. **O relatório do que *não* foi convertido acompanha a exportação**, sempre —
   é a informação de que o usuário precisa para terminar o trabalho à mão, e
   ele sai de graça dos nós `Unsupported` de US-8, dos conflitos de US-7 e da
   cobertura de prova de US-10. Não é um recurso novo; é a agregação de três
   coisas que já existem.
3. **Teste de independência** (a condição de testabilidade acima): copiar o
   pacote exportado para um diretório temporário fora do projeto, rodar
   `dart pub get`, `dart analyze` e `dart test` ali, e afirmar que passa. É o
   único teste que prova que a exportação não depende do ambiente de origem.
4. **Portal de arquivos do Flatpak** para escolher o destino: o `sandbox` não
   permite caminho arbitrário. Isolar isso atrás de uma interface no cliente,
   para que o teste possa substituí-la por um diretório temporário.

---

## US-12 — Re-ingestão preservando decisões

**Status:** planejado · **Depende de:** US-7

### Objetivo do usuário

Atualizar o código C++ de entrada sem perder o trabalho de decisão já feito.

### Observações e decisões em aberto

- É aqui que a **identidade estável de tipo** de US-3 se paga. Com chave por
  arquivo e linha, qualquer edição invalida todas as decisões.
- Definir a apresentação do diff: tipos novos, removidos, alterados, e decisões
  que deixaram de ser válidas.
- Definir o que acontece com traces de US-6 de funções modificadas —
  provavelmente invalidados e recaracterizados.
- Reprocessamento incremental (só o que mudou) é o que torna o ciclo usável em
  projetos grandes.

### Critérios de aceitação (testáveis)

1. Re-ingerir um input idêntico preserva 100% das decisões.
2. Re-ingerir um input com um tipo renomeado sinaliza a decisão órfã em vez de
   descartá-la silenciosamente.
3. Re-ingerir um input com um tipo novo preserva as decisões dos demais.
4. Decisões de tipos cujo código mudou são marcadas para revisão.

### Condições de testabilidade

- Requer dois fixtures versionados representando "antes" e "depois", com as
  mudanças escolhidas para exercitar cada critério acima. A escada de exemplos
  dá isso de graça: duas versões de um mesmo exemplo, em texto puro, com diff
  legível na revisão.

### Roteiro de implementação (para um agente)

1. **Diff por `usr`, não por posição.** Comparar o catálogo novo com o gravado
   produz quatro conjuntos: novos, removidos, inalterados e **alterados**.
   "Alterado" precisa de um critério explícito — recomendo *hash* do texto do
   corpo, delimitado por `end_line`/`end_column`, que US-3 já persiste. Sem
   isso, "alterado" vira "qualquer coisa que mudou de linha", e o critério 4
   dispara para o projeto inteiro a cada re-ingestão.
2. **Nada é descartado silenciosamente** (critério 2): uma decisão cujo `usr`
   sumiu vira decisão órfã, listada com o nome do tipo que ela tinha, para o
   usuário reatribuir ou descartar. Mesma mecânica dos traces órfãos de US-6.1
   — vale implementar as duas com o mesmo conceito, não com dois parecidos.
3. **Marcação para revisão** (critério 4) atinge três coisas ao mesmo tempo:
   decisões de US-7, seleções de US-6.1 e traces de US-6.5. Todas chaveadas por
   `usr`, todas com o mesmo estado "válida, mas pendente de revisão".
4. **Incrementalidade é o item mais caro e o mais adiável.** Reprocessar só as
   TUs alteradas é o que torna o ciclo usável no Verovio, e é a mesma lacuna que
   US-3, US-4 e US-5 já registram. Recomendo tratá-la como trabalho próprio,
   depois que o diff e a preservação de decisões estiverem provados — refazer
   as três passadas inteiras é lento, mas correto; um cache incremental errado
   é rápido e corrompe as decisões, que são o ativo mais valioso do produto.
5. **Rota e UI:** `POST /projects/reingest` devolvendo `job_id` (quarta ou
   quinta instância do mecanismo de `jobs.rs`), e uma tela de diff mostrando os
   quatro conjuntos antes de o usuário confirmar.

---

## Observações transversais ainda em aberto

Estes pontos atravessam vários passos e não pertencem a nenhum deles isolado.
Nenhum está resolvido hoje.

### Modelo intermediário

O `AGENTS.md` exigia a fronteira "modelo intermediário", e o fluxo acima salta de
análise (US-3 a US-5) para mapeamento (US-7) sem nomeá-lo. É a peça que sustenta
a extensibilidade a outras linguagens de entrada e saída — sem ela, cada
adaptador novo reescreve o produto inteiro. Precisa existir antes de US-8, e a
decisão de projetá-lo agora ou depois deve ser consciente.

Resposta à observação: Retirei do 'AGENTS.md' o tal 'modelo intermediário'. 

**Resolvido (Q8): a IR deixou de ser fronteira exigida, mas continua existindo
como estrutura interna de US-8** — leitura (2), a recomendada. A leitura
descartada era (1) "não haverá IR alguma", com o emissor de US-8 lendo os
catálogos e os cursores do `libclang` e escrevendo Dart direto: chegaria mais
rápido ao primeiro Dart gerado, mas o emissor passaria a conhecer C++ e Dart ao
mesmo tempo, e um segundo par de linguagens exigiria reescrevê-lo inteiro.

O que a decisão significa na prática:

- A IR é a que `conversao-guiada-por-exemplos.md` §7 já propõe concretamente —
  `crates/server/src/ir/`, dimensionada pelo que cada degrau da escada exige,
  nascendo com oito nós no E01. **Não é para ser projetada em largura antes do
  E01**; ela cresce degrau a degrau, como o resto da escada.
- Ela é estrutura interna, não contrato: não aparece em rota HTTP, não é
  persistida como formato de intercâmbio, e mudá-la não quebra cliente algum.
  Retirá-la do `AGENTS.md` foi retirar a *exigência de fronteira*, não a
  estrutura.
- O motivo decisivo é de teste, não de arquitetura: sem uma estrutura
  intermediária, não existe nada que o teste de US-8 possa afirmar entre "o C++
  entrou" e "o Dart saiu" — todo teste viraria comparação de texto gerado, e a
  regra de ouro do `AGENTS.md` ficaria difícil de cumprir em qualquer
  granularidade menor que o arquivo inteiro.
- A diferença entre (1) e (2) é pequena no E01 e grande no E05, quando o
  adaptador de biblioteca padrão precisa ser uma tabela substituível e não uma
  cascata de condicionais dentro do emissor.
- O `AGENTS.md` continua exigindo fronteiras claras entre análise de entrada e
  geração de saída; a IR interna é o que sustenta essa exigência sem
  reintroduzir a fronteira retirada.

### Extensibilidade por adaptador

Todo passo acima está escrito em termos de C++, CMake e Dart. Vale marcar, em
cada um, o que é adaptador de linguagem e o que é núcleo. Candidatos a
adaptador: descoberta de build (US-1), extração de catálogo (US-3, US-5),
regras de mapeamento (US-7), emissão (US-8), validação de saída (US-9).

### Contrato cliente/servidor

O documento descreve UX, mas o sistema é cliente-servidor e já sofreu um bug de
contrato divergente (`build_layers`, registrado em `docs/code-quality.md`). Cada
passo deveria declarar as rotas e os formatos que introduz, e o contrato deveria
ter fonte única em vez de dois modelos escritos à mão em linguagens diferentes.

### Trabalho longo: progresso, cancelamento, incrementalidade

As rotas em geral ainda são síncronas. A partir de US-4 isso deixa de
funcionar.

**Primeira instância resolvida:** `POST /projects` (US-1) — job em memória
(`crates/server/src/jobs.rs`), sem identificador persistido, com
`GET /projects/jobs/{id}` para consulta de progresso. Cancelamento **não**
foi resolvido por essa instância (não há como cancelar um job de criação em
andamento) nem o comportamento transacional correspondente — só a parte de
progresso. O modelo (um registro de jobs em memória, progresso derivado de
contadores atômicos em vez de estado explícito) fica como precedente para
US-4 e US-6 decidirem se reaproveitam ou não.

**US-4 reaproveitou em vez de decidir de novo:** em vez de um job próprio, a
indexação de usos roda dentro da *mesma* passada `libclang` de US-3 (mesma
varredura de AST, populando um terceiro vetor), então herda de graça o
progresso já relatado por `GET /projects/jobs/{id}` para `type_catalog` — sem
rota nem contador novos.

**Cancelamento resolvido, por US-4, para o mecanismo de job em geral.**
US-1 tinha deixado cancelamento em aberto (só progresso estava resolvido);
US-4 fechou essa lacuna para o próprio mecanismo compartilhado, não só para
usos: `progress::Cancellation` (um `AtomicBool` checado uma vez por unidade de
compilação, em `type_catalog::parse_chunk` e `source_catalog::parse_chunk`) e
`DELETE /projects/jobs/{job_id}` cancelam a criação de projeto inteira — a
extração de tipos (US-3), a indexação de usos (US-4) e a descoberta de
arquivos fonte (US-2) param juntas, já que todas vivem dentro do mesmo job.
Como consequência, US-1 também ganhou cancelamento de fato, embora a decisão
tenha sido tomada e documentada aqui. Ver US-4 para os detalhes de
implementação e teste. US-6 segue em aberto quanto a reaproveitar esse modelo
ou não; ao contrário de US-4, uma caracterização comportamental *tem* uma
noção de "meio caminho seguro para persistir" que uma indexação de tipos não
tem, então pode não fazer sentido herdar a mesma solução sem adaptação.

**US-5 reaproveitou só a metade que fazia sentido.** Ao contrário de US-4 (que
reaproveitou a *passada* inteira de US-3, já que usos de tipo cabem na mesma
varredura sem corpos de função), o grafo de chamadas de US-5 só existe dentro
de corpos de função — reaproveitar a AST de US-3/US-4 era impossível, então
`function_catalog::extract_function_catalog_cancellable` é uma terceira
passada `libclang` independente (ver US-5 para o detalhe). O que *foi*
reaproveitado é exatamente o mecanismo de job desta seção:
`CreationProgress` ganhou um terceiro `ExtractionProgress`
(`function_catalog`), e `progress::Cancellation`, já compartilhado entre as
duas primeiras passadas, passou a ser checado pela terceira também, sem
nenhum flag novo — `DELETE /projects/jobs/{job_id}` continua parando a
criação de projeto inteira, agora com uma passada a mais dentro dela.

**US-6.3 (fase A) será a quarta instância, e a primeira de um tipo diferente.**
As três anteriores são trabalho de *análise*: interrompê-las no meio não custa
nada, porque nada foi persistido até o fim. Compilar o projeto instrumentado e
executar perfis não é assim — há artefatos de build no disco e traces já
coletados quando o cancelamento chega. A observação registrada no parágrafo
anterior sobre US-6 ("uma caracterização comportamental *tem* uma noção de meio
caminho seguro para persistir, que uma indexação de tipos não tem") se
materializa exatamente aqui, e a decisão que ela pedia é: **traces já
coletados sobrevivem ao cancelamento**, gravados sob uma execução com
`status = "cancelled"`, e o diretório de build sobrevive também (jogá-lo fora
obrigaria a recompilar tudo na próxima tentativa). O mecanismo continua sendo o
mesmo `progress::Cancellation`; o que muda é o que se faz ao observá-lo.

### Versionamento do esquema de persistência

Não há versão de esquema nem migração genérica. Os passos seguintes vão
alterar tabelas existentes, e projetos criados por versões anteriores
precisam de um caminho — nem que seja detectar e recusar com mensagem clara.

A adição de `namespace`/`end_line`/`end_column` a `type_declarations` e
`type_dependencies` (US-3) quebrou reabertura de projetos existentes em teste
manual (`no such column: namespace`) antes de `ProjectStore::open` ganhar uma
migração pontual (`migrate_type_columns`, via `PRAGMA table_info` +
`ALTER TABLE ADD COLUMN`, testada em
`opening_a_pre_namespace_database_adds_the_missing_columns`). Isso resolve
*esse* caso, mas não é o mecanismo geral que este item pede — a próxima coluna
nova exige o mesmo trabalho manual de novo.

### Escala

Falta um alvo declarado de tamanho de projeto suportado (número de TUs, linhas,
tipos). O fixture Verovio já é uma boa referência; sem um alvo escrito, decisões
de arquitetura em US-4 e US-6 ficam sem critério.

Um relato de "trava no meio da importação" ao importar o Verovio 5.7.0 real
(não o fixture de 6.2.0 já versionado) confirmou esse risco na prática, não só
em teoria: `project_service::create_project` faz **duas** passadas completas de
`libclang` sobre as 291 unidades de compilação — uma em
`type_catalog::extract_type_catalog`, outra, independente, em
`source_catalog::extract_source_files` (`libclang` não expõe como reaproveitar
a AST já parseada entre as duas) — cada `parse` frio levando ~1.3s por arquivo,
sem nenhum log de progresso em nenhuma das duas. Resultado: 6+ minutos de
silêncio total por passada, dentro de uma requisição HTTP síncrona, o que é
indistinguível de travamento do ponto de vista do usuário.

Reproduzido por um teste dedicado, não executado por padrão —
`crates/server/tests/verovio_5_7_0_import_diagnosis.rs`
(`cargo test -p syntax-bridge-server --test verovio_5_7_0_import_diagnosis --
--ignored --nocapture`), contra o arquivo real
`test-resources/verovio-version-5.7.0.tar.gz`.

Mitigado, não resolvido: as duas passadas agora logam progresso por unidade
(`log_type_catalog`/`[type_catalog]` no log do servidor) e paralelizam entre
si — um `CXIndex` por thread de trabalho (um índice por thread é o único jeito
documentado de paralelizar `libclang` com segurança; cada thread precisa
carregar a biblioteca dinamicamente de novo, já que o carregamento do
`clang-sys` com a feature `runtime` é por thread). Isso cortou o tempo total de
importação do Verovio 5.7.0 de mais de 300s (o teste chegou a estourar esse
timeout duas vezes antes da paralelização) para ~203s neste ambiente de 4
núcleos — proporcional ao número de núcleos disponíveis, não uma correção
estrutural. Um projeto ainda maior, ou uma máquina com menos núcleos (o
sandbox Flatpak, por exemplo), volta a bater no mesmo problema. A correção de
verdade é a já registrada acima em "Trabalho longo": tirar a ingestão da
requisição HTTP síncrona e dar progresso real ao usuário.

**US-5 piora este número de propósito, por uma razão específica.** A
extração do grafo de chamadas precisa dos corpos de função, que as duas
passadas acima deliberadamente pulam (`CXTranslationUnit_SkipFunctionBodies`)
para ficarem rápidas — não há meio-termo em `libclang`: ou os corpos são
parseados (caro) ou o grafo de chamadas simplesmente não existe. `create_project`
agora faz uma **terceira** passada completa, com corpos, do mesmo tamanho de
custo por arquivo que motivou a otimização das outras duas. É um trade-off
consciente (documentado no módulo `function_catalog.rs`), não uma regressão
não percebida, mas soma ao mesmo risco de escala desta seção — um projeto que
já era lento com 2 passadas fica mais lento ainda com 3, e a paralelização por
núcleo é a mesma mitigação parcial, com o mesmo limite.

### Preprocessing record de macros — decisão: não abordar agora

US-3, US-4 e US-5 documentam, cada um separadamente, uma lacuna com uma única
causa raiz: o `libclang` separa preprocessamento (onde macros vivem) da AST
(onde os três passes atuais operam), e nenhum deles consulta o
*preprocessing record*. Concretamente:

- US-3: macro de compilação condicional (`#ifdef`/`#if` fora de guarda de
  header) não tem representação própria, cai genericamente em
  `AnnotationMacro`.
- US-4: uso de tipo dentro de uma expansão de macro não gera `TypeUsage`.
- US-5: chamada dentro de uma expansão de macro-função não gera aresta no
  grafo de chamadas.

**Decisão:** essa lacuna não será abordada agora. Resolvê-la de uma vez
(consultar o *preprocessing record*) destravaria as três simultaneamente, mas
nenhuma delas bloqueia os próximos passos do roadmap (US-6 em diante). Fica
registrada aqui como trabalho futuro, não como pendência ativa — os três
pontos abaixo remetem a esta seção em vez de repetir a decisão.

### Erros e diagnósticos

Não há política de como falhas de parse, de build, de KLEE ou do analisador Dart
chegam ao usuário. A distinção erro-de-cliente/erro-de-servidor já existe no
servidor e deveria se estender a diagnósticos de análise.

### Segurança

O produto compila e executa código arbitrário de terceiros (US-1 e, sobretudo,
US-6). A postura precisa estar escrita: o que roda dentro do sandbox, que acesso
a rede e a disco existe, e o que é recusado.

**Escrita, para US-6, em US-6.5** ("Segurança"): tudo dentro do sandbox do
Flatpak, tudo gravando exclusivamente sob `<projeto>/characterization/`, sem
rede — com a limitação declarada de que timeout e teto de disco mitigam, mas não
eliminam, um input que consuma recursos dentro do sandbox. Falta estender a
mesma redação a US-1 (o `cmake configure` de um projeto de terceiros já executa
código arbitrário, via `execute_process` em `CMakeLists.txt`, e isso nunca foi
declarado) e a US-11 (a exportação é a única operação que legitimamente escreve
fora do diretório do projeto, e por isso precisa do portal de arquivos).

### Ambiente de teste

O `AGENTS.md` exige rodar os testes dentro do Flatpak. Hoje o manifesto oferece
`rust-stable`, `llvm21`, o **Dart SDK 3.12.2** (módulo `dart-sdk`, instalado em
`/app/lib/dart-sdk` com `/app/bin/dart` no caminho) e, desde 2026-08-13, o
**GoogleTest v1.18.0** (módulo `googletest`, construído via CMake, instalado em
`/app/lib`/`/app/include` — decisão explícita do usuário revendo Q10). **Só
`klee` continua fora dele.**

Consequência por passo:

- **US-9 e US-11 são testáveis no ambiente de destino desde já** — o que
  precisavam era o Dart SDK, e ele está lá, com versão fixada por `sha256`.
- **US-6 fase A é testável no ambiente de destino desde já.** É a mudança que
  as respostas da rodada 1 produziram: a via de execução real precisa de
  `cmake`, `clang++` e, para medir cobertura, `llvm-profdata`/`llvm-cov` — os
  três já vêm da extensão `llvm21` e do runtime, nenhum é novo no manifesto.
- **US-6 fase B está adiada, não apenas bloqueada** (Q10 respondida, revista
  parcialmente): GoogleTest (para materializar e executar os casos) já entrou
  no manifesto, mas KLEE (para descobrir entradas que cobrem todos os ramos)
  continua fora, e é ele quem ainda impede a via sintética — GoogleTest
  sozinho não descobre entrada nenhuma. A inclusão de KLEE volta à mesa quando
  a fase A tiver rodado sobre o Verovio e `llvm-cov` mostrar, com número
  medido, quanta cobertura ela deixa de fora. Ver **Q10** em US-6.4.
- **US-10 não herda mais bloqueio nenhum**: o oráculo pode vir dos casos
  escritos à mão da escada de exemplos ou dos `behavior_traces` da fase A de
  US-6, no mesmo formato, e rodar os testes Dart gerados já é possível.

Ou seja: a dependência de infraestrutura mais importante do roadmap encolheu de
"três ferramentas ausentes bloqueando quatro passos" para "uma ferramenta
ausente (`klee`) bloqueando **meio** passo" — a fase sintética de US-6 —, e essa
fase tem contorno conhecido e não bloqueia mais nada a jusante.

Uma capacidade nova entra no ambiente com US-6.3, e vale registrar aqui porque
não é ferramenta ausente e sim uso ausente: o produto passa a **compilar** o
projeto de entrada (`cmake --build`), coisa que hoje nunca faz — só configura.
Ver Q7 (respondida: só os alvos necessários ao perfil, dentro do mecanismo de
job de US-1/US-4).

### Higiene do repositório

Resolvido: `test-resources/Test-package/` (saída de execução, com `build/` e
`project.db`) foi removido do repositório, e `test-resources/*/build/` entrou
no `.gitignore`. O único fixture leve em `test-resources/` hoje é
`sample-cmake-project.tar.gz`, versionado. O fixture *combinado* do critério
de testabilidade de US-3 (struct, herança, union, enum, typedef, alias, macro,
namespace e homônimos, todos no mesmo projeto) ainda não existe — é o que
falta para o critério 1 de US-3 ser exercitável em um único teste. O critério
3 (homônimos em namespaces distintos) já tem um fixture próprio, menor, em
`crates/server/tests/type_catalog.rs`
(`create_project_catalogs_namespace_and_extent_with_libclang`).
