# Prompt: catálogo de ponteiros + solver com fluxo de tipos (TFA)

**Status (2026-08-15): Parte 1 completa; Parte 2 com narrowing intra e
interprocedural (return-forwarding), já servida em produção via
`GET /projects/pointers`.** Parte 1 ponta a ponta:
`crates/server/src/pointer_catalog.rs` (extração), `pointer_declarations`
em `project_store.rs` (persistência), `GET /projects/pointers` (rota),
`PointersView` + painel dockável "Pointers" no cliente Flutter com teste de
screenshot (`pointer_catalog_screenshots_test.dart`), e testes
`crates/server/tests/pointer_catalog.rs`/`pointer_catalog_route.rs`. Parte 2
tem dois casos do corpus — B07 (evidência de construção no próprio corpo,
`mapping-solver-fixtures/B07-ponteiro-com-atribuicao-unica/`) e B08
(encaminhamento entre funções via `return Callee(...)`, seguindo
`function_catalog::CallEdge`, `mapping-solver-fixtures/B08-fabrica-delegada/`)
— e a narrowing correspondente em `mapping::pointer_options_for` /
`narrow_by_construction_evidence`, sound por padrão em ambos os casos (nunca
estreita sem evidência, nunca segue um ciclo de encaminhamento).
`project_service::list_pointers` reconstrói um `mapping::ProjectFacts`
completo a partir do que já está persistido (sem reparsear — novos métodos
`ProjectStore::list_type_usages`/`list_call_edges`, sem filtro, para isso) e
chama o solver de verdade para cada ponteiro de retorno, expondo o
resultado como `possible_types` na mesma resposta de `GET /projects/pointers`
— provado ponta a ponta em
`list_pointers_narrows_return_type_pointers_using_the_persisted_call_graph`
(`crates/server/tests/pointer_catalog.rs`), que ingere um projeto real com
fábrica direta + fábrica delegada e confirma que ambas as funções narrowing
para `{Triangulo}`, não o CHA completo `{Forma, Triangulo, Quadrado}`.
Ainda não cobre parâmetro/campo/local (só `return_type`, ver limitação
abaixo), e o cliente Flutter ainda não exibe `possible_types` — a UI
continua mostrando só o catálogo bruto de Parte 1.

**Avaliado e deliberadamente não construído** (revisão de 2026-08-15, contra
os itens levantados quando a Parte 1/primeiro corte da Parte 2 ficaram
prontos):

- **Ligar `pointer_catalog` (Parte 1) à narrowing (Parte 2) exigiria uma
  extração nova, não só reusar a existente.** A narrowing lê o
  código-fonte direto (`read_span`); `pointer_catalog::PointerDeclaration`
  só guarda *declarações*, não *sites de atribuição* (onde um ponteiro
  recebe um valor). Para a narrowing consultar o catálogo em vez de
  reler o arquivo, o catálogo precisaria de um novo tipo de fato —
  correlacionar cada `CXXNewExpr`/`CallExpr`/`ReturnStmt` ao ponteiro que
  recebe seu valor via `clang_getCursorSemanticParent` — que é uma
  extração via `libclang` genuinamente nova, do tamanho de
  `pointer_catalog.rs` inteiro de novo. B07/B08 não pedem isso: o texto
  já resolve os dois. Construir a extração de sites de atribuição sem um
  caso do corpus que prove que o texto não basta seria especulação —
  o mesmo "não implementação especulativa" que motivou B07/B08 em primeiro
  lugar. Fica registrado aqui como o próximo passo natural, não construído.
**Atualização (2026-08-15): o solver geral de US-7 ganhou uma primeira fatia
real, fora do escopo original de ponteiros deste plano, mas registrada
aqui porque nasceu do mesmo método.** `mapping::feasible_options` (caso B09,
`docs/mapping-solver-cases.md`) é o primeiro caso em que uma opção de
`options_for` para um tipo é substituída porque *outro* tipo do projeto já
força uma exigência incompatível — critério 3 de US-7 aplicado de verdade
pela primeira vez, não só descrito. Continua sendo só isso: um caso, uma
regra. O solver de satisfação de restrições completo (comparável em
tamanho ao resto do servidor, "o item mais caro" do roteiro) continua fora
de escopo — B09 não é esse solver, é a prova de que o mecanismo funciona
para uma forma concreta de conflito.

**Achado incidental, corrigido (2026-08-15): `function_catalog.rs` perdia
por completo chamadas a uma função livre sobrecarregada quando o resultado
alimentava outra chamada** (`formatar(contagem) + " / " + formatar(media)`,
caso B04) — `libclang` expõe esse padrão como um `CXCursor_OverloadedDeclRef`
solto, sem `CXCursor_CallExpr` ao redor, e `clang_getCursorReferenced` nele
não resolve para nada útil. `record_overloaded_call` desambigua comparando
o tipo do argumento (cursor seguinte, empiricamente confiável para esse
padrão) contra o único parâmetro de cada candidato, resolvendo quando exatamente
um bate e marcando não resolvido (nunca um palpite) quando não. Não fazia
parte deste plano — apareceu ao investigar por que `b04_overload_rename_...`
já estava vermelho antes desta sessão começar; corrigido porque o usuário
pediu diretamente ("trate o b04").

## Contexto

O solver de mapeamento (`crates/server/src/mapping.rs`) decide hoje o
mapeamento de um ponteiro C++ (`pointer_options_for`, `possible_pointee_types`,
mapping.rs:1050-1137) subindo a hierarquia de classes do tipo apontado e
enumerando *toda* subclasse alcançável (class hierarchy analysis, CHA) —
sound, mas superestima o conjunto sempre que a hierarquia é maior do que os
usos reais, porque nunca olha se algum código de fato atribui aquela
subclasse àquele ponteiro específico. Além disso, a detecção de "isto é um
ponteiro" no nível de assinatura ainda é textual (`signature.contains('*')`,
mapping.rs:905), porque nem `type_catalog` nem `function_catalog` expõem
ponteiros como fatos estruturados hoje.

Este trabalho tem duas partes que devem ser feitas nessa ordem — a segunda
depende dos fatos que a primeira extrai:

1. Um catálogo de ponteiros declarados no projeto, recurso de produto
   completo — mesmo nível de `type_catalog`/`source_catalog`/US-3/US-2 — não
   apenas uma estrutura interna de teste.
2. A evolução do solver de ponteiros de CHA para uma análise de fluxo de
   tipos (TFA/DFA): usar o grafo interprocedural caller/callee que
   `function_catalog` já expõe (`CallEdge`/`CallResolution`,
   function_catalog.rs:138-155 — já construído pensando nesse uso) somado aos
   sites de atribuição que o novo catálogo de ponteiros vai expor, para
   estreitar o conjunto de tipos possíveis além do que CHA sozinho enumera.

Siga TDD e o método deste repositório (`AGENTS.md`): toda mudança de
comportamento começa com um teste que falha. Rode `just test` (dentro do
Flatpak) ou, se o Flatpak não estiver disponível, `just test-host` — registre
no resumo final o que rodou fora do Flatpak.

## Parte 1 — catálogo de ponteiros

Objetivo: uma nova lista, análoga a `type_catalog`/`source_catalog`, com todo
ponteiro declarado no projeto (parâmetros, membros, locais, retornos) —
localização, tipo apontado, e onde/como ele é declarado. Espelhe o padrão que
`type_catalog.rs` e `function_catalog.rs` já seguem, ponta a ponta:

- **Extração**: novo módulo `crates/server/src/pointer_catalog.rs`, usando
  `libclang` via `crate::ingest::CompilationUnit` do mesmo jeito que
  `type_catalog::extract_type_catalog_cancellable` faz (paralelização por
  `CompilationUnit`, `Cancellation`/`ExtractionProgress`, `libclang` carregado
  dinamicamente — não hardcode nenhum path). Antes de desenhar o schema,
  decida e documente no próprio módulo (comentário de topo, como
  `type_catalog.rs` faz) o que conta como "ponteiro declarado": parâmetro
  `T*`, campo, variável local, retorno de função — inclua todos; ponteiro de
  função (`T (*)(...)`) e ponteiro duplo (`T**`) são casos distintos que
  precisam de kind próprio, não fallback silencioso; `std::unique_ptr`/
  `std::shared_ptr` ficam de fora desta primeira versão (não são `*` na
  sintaxe, e o solver já trata `std::vector`/`std::string` como adaptadores
  de biblioteca à parte — mesma lógica se aplicaria a smart pointers depois,
  não agora).
- **Progresso**: adicione o campo equivalente em `crate::progress` (ver como
  `type_catalog`/`source_catalog` já aparecem em `progress.rs:1-16`) e
  encadeie em `jobs.rs` como as outras duas passes.
- **Persistência**: `crate::persistence::project_store` já persiste
  `TypeCatalog`/`SourceFile` (ver imports em `project_store.rs:13-14`) —
  adicione o equivalente para o catálogo de ponteiros, mesmo padrão de
  save/load.
- **Rota HTTP**: `server.rs:159-161` expõe `/projects/types`,
  `/projects/types/usages`, `/projects/functions`. Adicione
  `/projects/pointers` (e, se fizer sentido para navegação, um endpoint de
  "usos" análogo a `/projects/types/usages`) seguindo o mesmo formato de
  handler.
- **Teste de rota**: espelhe `crates/server/tests/function_catalog_route.rs`.
- **Cliente Flutter**: nova tela em `client/flutter/lib/src/ui/`, análoga a
  `types_view.dart`/`source_files_view.dart`. Por instrução de
  `AGENTS.md`, toda tela nova precisa de teste de screenshot em
  `client/flutter/test/screenshots/` (veja `us3_types_screenshots_test.dart`
  como modelo) cobrindo não só a tela final mas estados intermediários
  relevantes (lista vazia, carregando, erro, lista populada) — sem isso a
  tarefa não está concluída.
- **Registro no roadmap**: `docs/plans/User Steps.md` é a fonte única do
  roadmap. US-7 já é "Mapeamento de tipos C++ → Dart" — decida, ao escrever,
  se este catálogo é um pré-requisito documentado dentro de US-7 ou merece
  sua própria entrada; registre a decisão lá, não só no código.

## Parte 2 — solver: de CHA para TFA/DFA

Objetivo: estreitar `possible_pointee_types` (mapping.rs:1109-1137) usando
fluxo de tipos real, não só a hierarquia estática do tipo declarado.

- **Substrato interprocedural já existe**: `FunctionCatalog::calls`
  (`CallEdge`/`CallResolution`, function_catalog.rs:138-155) — inclui
  `is_dynamic_dispatch` (onde a resolução estática do libclang diverge do
  despacho virtual real) e `Unresolved { reason }` (cobre, entre outros
  casos, chamada por ponteiro de função — interseção direta com o catálogo
  da Parte 1).
- **Substrato intraprocedural é o que falta**: rastrear, dentro do corpo de
  cada função, quais tipos concretos são de fato atribuídos a cada
  ponteiro. Os *sites* de atribuição que o catálogo da Parte 1 precisa
  capturar (não só a declaração) são o fato de entrada dessa análise.
- **Método do corpus, não implementação especulativa**: antes de escrever a
  análise geral, ache (ou construa) em `mapping-solver-fixtures/` um caso
  concreto onde a superestimativa atual do CHA já dá uma resposta errada ou
  inutilmente ampla — ex.: um ponteiro para uma classe-base com várias
  subclasses no projeto, mas que só é atribuído, em todo o código-fonte, com
  instâncias de uma delas. Documente esse caso em
  `docs/mapping-solver-cases.md` seguindo o formato existente (índice, uma
  das três categorias — isto é claramente categoria B, "global": a resposta
  certa só aparece combinando a declaração do ponteiro com os sites de
  atribuição espalhados pelo projeto — próximo ID livre depois de B06).
  Escreva o teste em `crates/server/tests/mapping_solver_cases.rs` esperando
  o conjunto *estreitado*, veja-o falhar, e só então implemente.
- **Suporte a `A10` já existente**: `mapping-solver-fixtures/A10-ponteiro-
  para-classe-referencia-anulavel/` já cobre o caso base (ponteiro para tipo
  conhecido, sem mais fatos) — não regrida esse caso; a análise de fluxo deve
  ser um refinamento aditivo, não uma substituição da enumeração por
  hierarquia.
- **Nunca under-approximate**: mantenha a garantia de soundness que o
  comentário de `possible_pointee_types` já documenta (mapping.rs:1020-1025).
  Quando a análise de fluxo for inconclusiva (ex.: atribuição vinda de um
  parâmetro cujo chamador não é resolvível — `CallResolution::Unresolved`),
  caia de volta para a enumeração por hierarquia de hoje, nunca para um
  conjunto mais estreito do que o comprovadamente correto.

## Restrições gerais (de `AGENTS.md`)

- TDD sempre: teste vermelho antes de qualquer implementação.
- Use as receitas do `justfile` (`just test`, `just check`, `just lint`), não
  `cargo`/`flutter`/`dart` crus.
- Não introduza dependência externa nova sem justificar no contexto da
  arquitetura.
- Mantenha fronteiras claras entre extração, solver e persistência — o
  catálogo de ponteiros é extração pura; a lógica de fluxo entra em
  `mapping.rs`, não no extrator.
