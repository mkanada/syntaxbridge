import '../project/project_models.dart';

abstract class ServerClient {
  Future<ServerStatus> health();

  /// Starts project creation as a background job on the server (ingest plus
  /// `libclang` extraction, which can take minutes for a real project) and
  /// returns its job id immediately, to be polled with
  /// [pollCreateProjectJob] rather than waiting on the whole thing inline.
  Future<String> startCreateProject(CreateProjectInput input);

  /// A snapshot of a job started by [startCreateProject]: still running
  /// (with progress), or its terminal outcome.
  Future<ProjectCreationJobStatus> pollCreateProjectJob(String jobId);

  /// Requests cancellation of a job started by [startCreateProject] (US-4
  /// criterion 7). Best-effort and asynchronous: this returns once the
  /// server has accepted the request, not once the job has actually
  /// stopped — callers keep polling [pollCreateProjectJob] to observe the
  /// `cancelling` → `cancelled` transition.
  Future<void> cancelCreateProject(String jobId);

  /// The last 5 projects the app was used with, most recently opened first.
  Future<List<RecentProject>> listRecentProjects();

  /// Drops a project from the recent-projects list. Nothing on disk is
  /// touched; this only makes the app stop offering it, which is what a user
  /// wants after deleting or moving the project directory themselves.
  Future<void> forgetProject(String projectDir);

  /// Reloads a project directly from its own persisted data, without
  /// running ingest again. Used both to reopen a recent project and to
  /// import a project that already exists on disk from a prior ingest.
  Future<CreatedProject> openProject(String projectDir);

  /// Starts the "Analyse" step (item 2, `docs/prompts/2026-08-19-mudanca-interacao.md`)
  /// as a background job on the server — the same shape as
  /// [startCreateProject]: re-extracts and persists usages, dependencies,
  /// the call graph, IR, and the pointer catalog, which ingestion
  /// deliberately leaves out. Poll with [pollAnalyseProjectJob].
  Future<String> startAnalyseProject(String projectDir);

  /// A snapshot of a job started by [startAnalyseProject].
  Future<AnalysisJobStatus> pollAnalyseProjectJob(String jobId);

  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  });

  /// The type catalog already persisted for a project (US-3), together with
  /// each type's usage count (US-4), without reparsing.
  Future<TypeCatalogListing> listTypes(String projectDir);

  /// Every recorded usage of the type identified by [typeUsr] (US-3's stable
  /// identity), from the persisted index (US-4), without reparsing.
  Future<List<TypeUsage>> listTypeUsages({
    required String projectDir,
    required String typeUsr,
  });

  /// The function/method/macro catalog already persisted for a project
  /// (US-5), together with each function's caller count, without reparsing.
  Future<FunctionCatalogListing> listFunctions(String projectDir);

  /// Every recorded caller of the function identified by [functionUsr]
  /// (its stable `usr`), from the persisted call graph (US-5), without
  /// reparsing.
  Future<List<CallEdge>> listCallers({
    required String projectDir,
    required String functionUsr,
  });

  /// Every recorded call site within [file] (US-5 criterion 5's other
  /// direction), from the persisted call graph, without reparsing — what
  /// lets the source viewer offer "click a call, jump to its definition"
  /// for a file already open on screen.
  Future<List<CallEdge>> listCallsInFile({
    required String projectDir,
    required String file,
  });

  /// The pointer catalog already persisted for a project (Parte 1 of
  /// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`), without reparsing.
  Future<List<PointerDeclaration>> listPointers(String projectDir);

  /// Transpiles the project's free functions and `struct`s to Dart (US-8,
  /// E01–E03 scope) and returns the emitted package. Synchronous — these
  /// examples transpile in milliseconds, so there's no job/progress
  /// mechanism here yet (mirrors the server route's own design, see
  /// `docs/plans/primeiro-corte-e01-e03.md`).
  Future<TranspiledPackage> transpileProject(String projectDir);

  /// Transpiles the project the same way [transpileProject] does, then runs
  /// `dart analyze` against the result and returns every diagnostic,
  /// translated back to its C++ origin where one could be located (US-9,
  /// criterion 3).
  Future<List<DartDiagnostic>> validateProject(String projectDir);

  /// The effective "extern" set plus both live regexp rule lists
  /// (`docs/plans/lista-de-externos.md`), recomputed fresh from the
  /// persisted catalogs on every call — never materialized server-side.
  Future<ExternalListing> listExternals(String projectDir);

  /// Records (or updates) a manual external/not-external decision for one
  /// usr — the direct-marking action `TypesView`/`FunctionsView`'s per-row
  /// toggle sends.
  Future<void> markExternal({
    required String projectDir,
    required String usr,
    required bool external,
  });

  /// Sets or clears a whole file's persistent external mark (item 3,
  /// `docs/prompts/2026-08-19-mudanca-interacao.md`): every declaration
  /// currently in [file], and any declared there later, becomes external
  /// for as long as this mark stands — unlike [markTypeExternal], which
  /// still expands to a one-time snapshot of individual marks.
  Future<void> markFileExternal({
    required String projectDir,
    required String file,
    required bool external,
  });

  /// Same shape as [markFileExternal], expanding a type to itself plus
  /// every method it owns instead of a file to its contents.
  Future<List<String>> markTypeExternal({
    required String projectDir,
    required String typeUsr,
  });

  /// Adds a name-regexp rule (decision 6). Throws if [pattern] doesn't
  /// compile — never persisted invalid.
  Future<NameRegexRule> addNameRegex({
    required String projectDir,
    required String pattern,
  });

  Future<void> removeNameRegex({required String projectDir, required int id});

  /// Same shape as [addNameRegex], for path-regexp rules.
  Future<PathRegexRule> addPathRegex({
    required String projectDir,
    required String pattern,
  });

  Future<void> removePathRegex({required String projectDir, required int id});
}
