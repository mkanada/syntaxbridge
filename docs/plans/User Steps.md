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

## Índice

| ID | Passo | Status | Depende de |
| --- | --- | --- | --- |
| US-1 | Criação de projeto e ingestão do input | pronto | — |
| US-2 | Lista de arquivos fonte e leitura de conteúdo | pronto | US-1 |
| US-3 | Catálogo de tipos do projeto | pronto | US-2 |
| US-4 | Usos de cada tipo e navegação | parcial (falta taxonomia de nível de expressão e cancelamento) | US-3 |
| US-5 | Funções, métodos e macros, e seus usos | planejado | US-3 |
| US-6 | Isolamento e caracterização comportamental | planejado | US-5 |
| US-7 | Mapeamento de tipos C++ → Dart | planejado | US-4, US-5 |
| US-8 | Geração do código Dart | planejado | US-7 |
| US-9 | Validação estática do Dart gerado | planejado | US-8 |
| US-10 | Prova de equivalência comportamental | planejado | US-6, US-8 |
| US-11 | Exportação do projeto convertido | planejado | US-9, US-10 |
| US-12 | Re-ingestão preservando decisões | planejado | US-7 |

Este arquivo é a fonte única do roadmap. Os antigos `docs/plans/ingest.md` e
`docs/plans/separate-compilation-units.md` foram absorvidos por US-1 e US-6,
respectivamente, e removidos do repositório.

`docs/plans/ui-lists.md` é o complemento de interface: enquanto este documento
diz *o que* o usuário consegue fazer em cada passo, aquele diz *onde* cada lista
aparece na UI e o que a interface atual precisa mudar para sustentá-la.

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
  própria — hoje cai em `AnnotationMacro` — e o destino de cada subtipo em
  Dart (US-7) ainda não foi definido.
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

**Status:** parcial — extração, persistência, rotas e navegação na UI existem
para a taxonomia de nível de assinatura (ver decisão de taxonomia abaixo);
faltam usos de nível de expressão (*cast*, `sizeof`, `new`, argumento de
template) e cancelamento de indexação em projetos grandes (critério 7,
parcial) · **Depende de:** US-3 ·
**Implementação:** `crates/server/src/type_catalog.rs` (`TypeUsageKind`,
`TypeUsage`, `push_usage`, extensão de `visit_cursor`/
`record_member_dependency`), `crates/server/src/persistence/project_store.rs`
(tabela `type_usages`, `replace_type_usages`, `list_type_usages_for`,
`type_usage_counts`), `crates/server/src/project_service.rs`
(`TypeCatalogListing`, `list_type_usages`), `crates/server/src/server.rs`
(`GET /projects/types/usages`, `usage_counts` em `GET /projects/types`),
`client/flutter/lib/src/project/project_models.dart` (`TypeUsage`,
`TypeUsageKind`, `TypeCatalogListing`),
`client/flutter/lib/src/ui/types_view.dart` (coluna e ordenação por número de
usos), `client/flutter/lib/src/ui/usages_view.dart` (painel de usos),
`client/flutter/lib/src/ui/server_status_page.dart` (fiação entre os dois) ·
**Testes:** `crates/server/tests/type_catalog.rs`
(`extract_type_catalog_records_usages_across_the_defined_taxonomy`),
`crates/server/tests/type_catalog_route.rs`, testes inline em
`persistence/project_store.rs`,
`client/flutter/test/types_view_test.dart`,
`client/flutter/test/usages_view_test.dart`, `client/flutter/test/app_test.dart`

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
  precisão.
- **Segue em aberto: usos dentro de código não compilado.** Continua
  verdadeiro e sem solução — `#ifdef` desligado é invisível ao `libclang`.
- **Segue em aberto: cancelamento (critério 7, parcial).** Progresso é
  relatado de graça, porque a indexação de usos anda junto da passada de tipos
  de US-3 e reaproveita os mesmos contadores atômicos que ela já expõe via
  `GET /projects/jobs/{id}`. Cancelamento não foi resolvido — mesmo precedente
  deixado em aberto por US-1.

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
7. **Parcial:** projetos grandes (Verovio, ~290 TUs) são indexados com
   progresso real reportado (reaproveitando o contador de US-3), mas
   cancelamento não existe — o mesmo estado em que US-1 deixou o próprio
   mecanismo de job.

### Condições de testabilidade

- O fixture precisa ter contagens de uso *conhecidas e escritas no teste*, o
  que exige um fixture propositalmente pequeno e estável — mudanças nele
  quebram os testes, e isso é aceitável desde que ele seja versionado.
  **Satisfeito:** `USAGE_TAXONOMY_CPP` em `type_catalog.rs`, com localização
  calculada por busca textual (`locate_in`) em vez de contada à mão, para
  sobreviver a edições do fixture.
- Precisa existir um segundo fixture, maior, para testar progresso,
  cancelamento e tempo — com asserção sobre ordem de grandeza, não sobre
  duração exata, que não é reprodutível. **Não satisfeito:** nenhum teste
  dedicado de progresso/tempo em escala existe para usos especificamente
  (o fixture Verovio de US-1/US-3 já prova que a extração não regride em
  escala, mas não testa cancelamento, que continua inexistente).
- Consultas de leitura precisam ser testáveis sem executar a indexação:
  popular o banco diretamente e consultar. **Satisfeito:**
  `usages_route_returns_the_persisted_usages_for_a_type` e os testes de
  round-trip em `project_store.rs` populam `type_usages` diretamente.

---

## US-5 — Funções, métodos e macros, e seus usos

**Status:** planejado · **Depende de:** US-3

### Objetivo do usuário

Identificar todas as funções, métodos e macros do projeto e todos os seus usos,
com a mesma navegação imediata de US-4, indo da definição ao uso e vice-versa.

### Observações e decisões em aberto

- **"Resolver questões relativas a herança de classes" é um subprojeto**, não um
  item. Precisa ser quebrado em: métodos virtuais e `override`, sobrecargas,
  herança múltipla, métodos herdados não redefinidos, ponteiros para função,
  *callbacks*, e chamadas por despacho dinâmico.
- **Distinguir grafo de chamadas estático de chamadas resolvíveis.** Uma parte
  das chamadas não é resolvível estaticamente e a ferramenta precisa dizer isso
  em vez de escolher um alvo arbitrário — a informação "aqui há despacho
  dinâmico" é justamente o que importa em US-7.
- **Sobrecarga é o ponto de atrito com Dart**, que não a tem. Registrar assinatura
  completa (não só nome) desde este passo evita retrabalho em US-7 e US-8.
- **Funções `inline`, `constexpr`, `template` e geradas pelo compilador**
  (construtores, operadores implícitos) precisam de política explícita: entram
  no catálogo? São convertidas? São ignoradas?
- Compartilha com US-4 a mesma infraestrutura de índice; implementar as duas
  com mecanismos separados seria duplicação.

### Critérios de aceitação (testáveis)

1. O catálogo contém cada função livre, método, construtor, destrutor e macro
   do projeto, com assinatura completa.
2. Duas sobrecargas do mesmo nome aparecem como entradas distintas.
3. Uma chamada a método virtual através de ponteiro para a classe base é
   registrada e marcada como despacho dinâmico.
4. Um método herdado e não redefinido é atribuído à classe que o define.
5. Da definição de uma função é possível listar seus chamadores, e de uma
   chamada é possível ir à definição.
6. Chamadas não resolvíveis estaticamente aparecem marcadas como tal, e não
   omitidas.

### Condições de testabilidade

- O fixture precisa conter deliberadamente: uma hierarquia com método virtual
  redefinido e outro não redefinido, um par de sobrecargas, um ponteiro para
  função, e uma macro-função — cada um existe para tornar um critério acima
  verificável.
- Os números esperados de chamadores precisam ser pequenos o bastante para
  serem escritos à mão no teste.

---

## US-6 — Isolamento e caracterização comportamental

**Status:** planejado · **Depende de:** US-5

Este passo absorve o conteúdo do antigo plano de separação de unidades de
compilação, que era mantido em documento próprio.

### Objetivo do usuário

Que o Syntax Bridge documente, por execução real, como cada função se comporta:
gerar código isolado por função — a função em questão, instrumentada, mais as
definições mínimas necessárias para executá-la em todos os ramos — e gravar no
banco o comportamento observado (parâmetros de entrada, resultado, coleções
modificadas, efeitos).

### Observações e decisões em aberto

- **Granularidade do isolamento.** O plano antigo falava em isolar *unidades de
  compilação* (criando mocks de tudo o que cada uma precisa); este passo fala em
  isolar *funções*. São coisas diferentes e provavelmente ambas necessárias: a
  unidade de compilação é o que compila, a função é o que se caracteriza.
  Decidir se a função isolada é compilada dentro de uma TU sintética própria (com
  mocks) ou extraída da TU original.
- **Isolar "com as definições mínimas" é *program slicing*.** O mecanismo já
  existe parcialmente: o fecho transitivo sai do grafo `type_dependencies` de
  US-3 somado ao grafo de chamadas de US-5. O que falta é a política de corte —
  onde parar e substituir por *stub*.
- **Papel de cada ferramenta:** KLEE para descobrir entradas que cobrem todos os
  ramos; GoogleTest para materializar e executar os casos. Isso precisa estar
  escrito, porque hoje as ferramentas aparecem no `AGENTS.md` sem passo que as
  consuma.
- **Funções não puras são a maioria e o caso difícil:** I/O, estado global,
  alocação, ponteiros recebidos, tempo, aleatoriedade, concorrência. Definir
  quais categorias são caracterizáveis na v1 e quais são explicitamente
  marcadas como "não caracterizada, requer decisão humana".
- **Não determinismo e limites de execução:** timeout por função, o que fazer
  quando o KLEE não converge (a maioria dos casos reais), teto de caminhos
  explorados, e como isso é reportado sem parecer falha.
- **Segurança.** Este passo compila e executa código arbitrário vindo do input
  do usuário. Precisa de posição explícita sobre isolamento de execução — é o
  argumento mais forte a favor do Flatpak e deve estar escrito aqui, não
  presumido.
- **O resultado deste passo é o oráculo de US-10.** Sem essa frase, o passo
  parece documentação; com ela, fica claro que é a base da prova de equivalência
  da conversão. O esquema de gravação dos traces precisa ser projetado para essa
  comparação, não só para exibição.

### Critérios de aceitação (testáveis)

1. Para uma função pura simples do fixture, o código isolado gerado compila
   sozinho.
2. Os casos gerados cobrem todos os ramos dessa função, e a cobertura é medida,
   não presumida.
3. O comportamento observado (entradas, saída, efeitos) é gravado no banco e
   pode ser recuperado.
4. Executar a caracterização duas vezes sobre o mesmo código produz o mesmo
   registro.
5. Uma função com dependência não satisfazível é marcada como não caracterizada,
   com motivo, sem interromper as demais.
6. Uma função que entra em laço infinito é interrompida pelo timeout e
   registrada como tal.
7. Nenhuma etapa deste passo escreve fora do diretório do projeto.

### Condições de testabilidade

- Determinismo é pré-requisito: sem entradas fixadas e sem controle de tempo e
  aleatoriedade, o critério 4 é impossível e o passo inteiro deixa de ser
  testável.
- O fixture precisa incluir, de propósito, uma função pura, uma função com
  efeito colateral em global, uma que recebe ponteiro, e uma que não termina.
- Precisa haver um modo de execução com limites (tempo, memória, caminhos)
  configuráveis pelo teste, senão a suíte fica lenta ou intermitente.
- KLEE e gtest precisam estar disponíveis no ambiente Flatpak — hoje não estão
  no manifesto. Enquanto não estiverem, este passo não é testável no ambiente
  de destino.

---

## US-7 — Mapeamento de tipos C++ → Dart

**Status:** planejado · **Depende de:** US-4, US-5

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
- **Filtrar opções por viabilidade global é um problema de satisfação de
  restrições**, não uma verificação local: a escolha em A propaga por todo o
  grafo de tipos. Definir se o produto resolve isso de fato ou se apenas alerta
  sobre conflitos após a escolha — as duas coisas têm custo muito diferente.
- **As decisões do usuário são o ativo mais valioso do projeto** e precisam ser
  persistidas com identidade estável de tipo (ver US-3) para sobreviver a US-12.
- **Ordem de decisão importa:** decidir tipos folha antes de tipos que dependem
  deles reduz retrabalho; o grafo de US-3 já dá essa ordem.
- Falta definir o que é "código ponte": adaptador gerado, classe manual com
  TODO, ou `ffi`. Sem essa definição o último item do passo não é implementável.

### Critérios de aceitação (testáveis)

1. Uma classe C++ sem herança múltipla recebe um mapeamento direto para classe
   Dart, sem apresentar alternativas.
2. Uma classe com herança múltipla recebe pelo menos uma combinação
   classe+mixin viável, com as consequências descritas.
3. Uma opção que tornaria outro tipo do projeto não convertível não é oferecida
   (ou é oferecida marcada como conflitante, conforme a decisão acima).
4. Escolher uma opção e reabrir o projeto preserva a escolha.
5. Um tipo sem mapeamento possível recebe ao menos uma opção de código ponte.
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

---

## US-8 — Geração do código Dart

**Status:** planejado · **Depende de:** US-7

### Objetivo do usuário

Obter o código Dart correspondente ao projeto C++, a partir dos mapeamentos
decididos.

### Observações e decisões em aberto

- Depende da existência de um **modelo intermediário explícito** (ver eixos
  transversais): gerar Dart diretamente a partir de cursores do libclang
  amarraria o produto a C++ e violaria a fronteira exigida pelo `AGENTS.md`.
- Ordem de geração sai da ordem topológica do grafo de tipos de US-3; ciclos
  precisam de política própria.
- Definir o mapeamento de estrutura de projeto: arquivos, bibliotecas, `part`,
  diretórios, e o `pubspec.yaml` gerado.
- Definir o que acontece com o que não foi decidido em US-7: gera com TODO,
  omite, ou bloqueia a geração.
- Rastreabilidade: cada trecho Dart gerado deve apontar para sua origem C++, o
  que é o que permite a navegação e o diagnóstico em US-9 e US-10.

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

---

## US-9 — Validação estática do Dart gerado

**Status:** planejado · **Depende de:** US-8

### Objetivo do usuário

Saber que o Dart gerado é válido antes de tentar executá-lo, com os erros
apontando de volta para o C++ de origem.

### Observações e decisões em aberto

- Ferramentas: `dart analyze` e `dart format` sobre o pacote gerado. O Dart SDK
  ainda não está no manifesto Flatpak.
- Diagnósticos do analisador precisam ser traduzidos para a origem C++ pela
  rastreabilidade de US-8; um erro Dart sem essa correlação é inútil para o
  usuário.
- Definir se avisos do analisador bloqueiam ou apenas informam.

### Critérios de aceitação (testáveis)

1. O pacote gerado para o fixture passa em `dart analyze` sem erros.
2. O código gerado já está no formato de `dart format` (formatar não produz
   diferença).
3. Um erro do analisador é apresentado com o arquivo e a linha C++ de origem.

### Condições de testabilidade

- Dart SDK disponível no ambiente de teste — e, para valer, dentro do Flatpak.
- Versão do SDK fixada, senão a saída do analisador varia entre máquinas.

---

## US-10 — Prova de equivalência comportamental

**Status:** planejado · **Depende de:** US-6, US-8

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

---

## US-11 — Exportação do projeto convertido

**Status:** planejado · **Depende de:** US-9, US-10

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
  mudanças escolhidas para exercitar cada critério acima.

---

## Observações transversais ainda em aberto

Estes pontos atravessam vários passos e não pertencem a nenhum deles isolado.
Nenhum está resolvido hoje.

### Modelo intermediário

O `AGENTS.md` exige a fronteira "modelo intermediário", e o fluxo acima salta de
análise (US-3 a US-5) para mapeamento (US-7) sem nomeá-lo. É a peça que sustenta
a extensibilidade a outras linguagens de entrada e saída — sem ela, cada
adaptador novo reescreve o produto inteiro. Precisa existir antes de US-8, e a
decisão de projetá-lo agora ou depois deve ser consciente.

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
rota nem contador novos. Cancelamento continua **não resolvido**, no mesmo
estado em que US-1 deixou. US-6 segue em aberto quanto a reaproveitar esse
modelo ou não; ao contrário de US-4, uma caracterização comportamental *tem*
uma noção de "meio caminho seguro para persistir" que uma indexação de tipos
não tem, então pode não fazer sentido herdar a mesma solução sem adaptação.

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

### Erros e diagnósticos

Não há política de como falhas de parse, de build, de KLEE ou do analisador Dart
chegam ao usuário. A distinção erro-de-cliente/erro-de-servidor já existe no
servidor e deveria se estender a diagnósticos de análise.

### Segurança

O produto compila e executa código arbitrário de terceiros (US-1 e, sobretudo,
US-6). A postura precisa estar escrita: o que roda dentro do sandbox, que acesso
a rede e a disco existe, e o que é recusado.

### Ambiente de teste

O `AGENTS.md` exige rodar os testes dentro do Flatpak. Hoje o manifesto oferece
`rust-stable` e `llvm21`; **KLEE, GoogleTest e Dart SDK não estão nele**. Os
passos US-6, US-9, US-10 e US-11 não são testáveis no ambiente de destino
enquanto isso não mudar — essa é a dependência de infraestrutura mais
importante do roadmap e não aparecia em nenhum plano.

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
