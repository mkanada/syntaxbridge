class ServerStatus {
  const ServerStatus({required this.service, required this.status});

  factory ServerStatus.fromJson(Map<String, Object?> json) {
    return ServerStatus(
      service: json['service'] as String? ?? 'unknown',
      status: json['status'] as String? ?? 'unknown',
    );
  }

  final String service;
  final String status;
}

class RecentProject {
  const RecentProject({
    required this.name,
    required this.projectDir,
    required this.sourceLanguage,
    required this.targetLanguage,
    required this.lastIngestStatus,
    this.available = true,
  });

  factory RecentProject.fromJson(Map<String, Object?> json) {
    return RecentProject(
      name: json['name'] as String? ?? 'unknown',
      projectDir: json['project_dir'] as String? ?? '',
      sourceLanguage: json['source_language'] as String? ?? '',
      targetLanguage: json['target_language'] as String? ?? '',
      lastIngestStatus: json['last_ingest_status'] as String? ?? '',
      available: json['available'] as bool? ?? true,
    );
  }

  final String name;
  final String projectDir;
  final String sourceLanguage;
  final String targetLanguage;
  final String lastIngestStatus;

  /// Whether the project directory is still on disk. The server resolves this
  /// when listing, since the user can delete a project folder at any time
  /// without the app knowing.
  final bool available;
}

class CreateProjectInput {
  const CreateProjectInput({
    required this.name,
    required this.workspaceDir,
    required this.archivePath,
  });

  final String name;
  final String workspaceDir;
  final String archivePath;

  Map<String, Object?> toJson() {
    return {
      'name': name,
      'workspace_dir': workspaceDir,
      'archive_path': archivePath,
    };
  }
}

class CreatedProject {
  const CreatedProject({
    required this.name,
    required this.projectDir,
    required this.inputSourceDir,
    required this.compilationUnits,
    this.sourceFiles = const <SourceFile>[],
  });

  factory CreatedProject.fromJson(Map<String, Object?> json) {
    final unitsJson =
        json['compilation_units'] as List<Object?>? ?? const <Object?>[];
    final sourceFilesJson =
        json['source_files'] as List<Object?>? ?? const <Object?>[];

    return CreatedProject(
      name: json['name'] as String? ?? 'unknown',
      projectDir: json['project_dir'] as String? ?? '',
      inputSourceDir: json['input_source_dir'] as String? ?? '',
      compilationUnits: unitsJson
          .whereType<Map<String, Object?>>()
          .map(CompilationUnit.fromJson)
          .toList(),
      sourceFiles: sourceFilesJson
          .whereType<Map<String, Object?>>()
          .map(SourceFile.fromJson)
          .toList(),
    );
  }

  final String name;
  final String projectDir;
  final String inputSourceDir;
  final List<CompilationUnit> compilationUnits;
  final List<SourceFile> sourceFiles;
}

class CompilationUnit {
  const CompilationUnit({
    required this.directory,
    required this.file,
    this.command,
    this.arguments = const <String>[],
  });

  factory CompilationUnit.fromJson(Map<String, Object?> json) {
    final argumentsJson = json['arguments'] as List<Object?>? ?? const [];

    return CompilationUnit(
      directory: json['directory'] as String? ?? '',
      file: json['file'] as String? ?? '',
      command: json['command'] as String?,
      arguments: argumentsJson.whereType<String>().toList(),
    );
  }

  final String directory;
  final String file;
  final String? command;
  final List<String> arguments;
}

enum SourceFileKind {
  translationUnit,
  header;

  static SourceFileKind fromJson(String? value) {
    return switch (value) {
      'translation_unit' => SourceFileKind.translationUnit,
      'header' => SourceFileKind.header,
      _ => SourceFileKind.header,
    };
  }
}

class SourceFile {
  const SourceFile({required this.path, required this.kind});

  factory SourceFile.fromJson(Map<String, Object?> json) {
    return SourceFile(
      path: json['path'] as String? ?? '',
      kind: SourceFileKind.fromJson(json['kind'] as String?),
    );
  }

  final String path;
  final SourceFileKind kind;
}

/// Mirrors `TypeDeclarationKind` in `crates/server/src/type_catalog.rs`.
enum TypeDeclarationKind {
  struct,
  class_,
  union,
  enum_,
  typedef,
  typeAlias,
  constantMacro,
  functionMacro,
  headerGuard,
  annotationMacro;

  static TypeDeclarationKind fromJson(String? value) {
    return switch (value) {
      'struct' => TypeDeclarationKind.struct,
      'class' => TypeDeclarationKind.class_,
      'union' => TypeDeclarationKind.union,
      'enum' => TypeDeclarationKind.enum_,
      'typedef' => TypeDeclarationKind.typedef,
      'type_alias' => TypeDeclarationKind.typeAlias,
      'constant_macro' => TypeDeclarationKind.constantMacro,
      'function_macro' => TypeDeclarationKind.functionMacro,
      'header_guard' => TypeDeclarationKind.headerGuard,
      'annotation_macro' => TypeDeclarationKind.annotationMacro,
      _ => TypeDeclarationKind.struct,
    };
  }

  /// Short label shown next to a type's name in the catalog.
  String get label {
    return switch (this) {
      TypeDeclarationKind.struct => 'struct',
      TypeDeclarationKind.class_ => 'class',
      TypeDeclarationKind.union => 'union',
      TypeDeclarationKind.enum_ => 'enum',
      TypeDeclarationKind.typedef => 'typedef',
      TypeDeclarationKind.typeAlias => 'type alias',
      TypeDeclarationKind.constantMacro => 'constant',
      TypeDeclarationKind.functionMacro => 'function macro',
      TypeDeclarationKind.headerGuard => 'header guard',
      TypeDeclarationKind.annotationMacro => 'macro',
    };
  }

  /// Whether this kind is ever worth showing in the type catalog UI. Header
  /// guards and other valueless annotation macros are pure preprocessor
  /// plumbing (`#ifndef FOO_H` / `#define FOO_H`, `#define MYLIB_API`) with
  /// nothing to show a user.
  bool get isUserVisible =>
      this != TypeDeclarationKind.headerGuard &&
      this != TypeDeclarationKind.annotationMacro;

  /// Whether this kind belongs in the "Constants & macros" section rather
  /// than alongside struct/class/union/enum/typedef/type alias — a macro
  /// isn't a type, even when it carries a value.
  bool get isMacro =>
      this == TypeDeclarationKind.constantMacro ||
      this == TypeDeclarationKind.functionMacro;
}

/// One named type declared in the project (US-3): a struct, class, union,
/// enum, typedef, type alias, or macro `#define`.
class TypeDeclaration {
  const TypeDeclaration({
    required this.name,
    required this.kind,
    this.namespace = '',
    required this.file,
    required this.line,
    required this.column,
    this.endLine = 0,
    this.endColumn = 0,
    this.usr = '',
  });

  factory TypeDeclaration.fromJson(Map<String, Object?> json) {
    return TypeDeclaration(
      name: json['name'] as String? ?? '',
      kind: TypeDeclarationKind.fromJson(json['kind'] as String?),
      namespace: json['namespace'] as String? ?? '',
      file: json['file'] as String? ?? '',
      line: json['line'] as int? ?? 0,
      column: json['column'] as int? ?? 0,
      endLine: json['end_line'] as int? ?? 0,
      endColumn: json['end_column'] as int? ?? 0,
      usr: json['usr'] as String? ?? '',
    );
  }

  final String name;
  final TypeDeclarationKind kind;

  /// The chain of enclosing namespaces, innermost last, joined with `::`
  /// (e.g. `"geometry::detail"`), or empty for a type declared at global
  /// scope.
  final String namespace;
  final String file;
  final int line;
  final int column;

  /// The line/column of the end of the declaration's extent (e.g. the
  /// closing `}` of a struct/class/union/enum body), for highlighting the
  /// whole declaration when opening its source file.
  final int endLine;
  final int endColumn;

  /// `libclang`'s Unified Symbol Resolution for this declaration — a
  /// semantic identity independent of source position, unlike
  /// `(kind, name, file, line, column)`, which breaks the instant an
  /// unrelated edit shifts line numbers. Mirrors `TypeDeclaration::usr` in
  /// `crates/server/src/type_catalog.rs`.
  final String usr;
}

/// Mirrors `TypeUsageKind` in `crates/server/src/type_catalog.rs`: the
/// closed taxonomy of "use" US-4 tracks. Only signature-level kinds are
/// covered — see the doc comment on the Rust enum for why (function bodies
/// aren't parsed, for the same performance reason `SkipFunctionBodies` was
/// introduced for US-1's Verovio-import fix).
enum TypeUsageKind {
  variableDeclaration,
  parameter,
  field,
  returnType,
  inheritance,
  typedefMention;

  static TypeUsageKind fromJson(String? value) {
    return switch (value) {
      'variable_declaration' => TypeUsageKind.variableDeclaration,
      'parameter' => TypeUsageKind.parameter,
      'field' => TypeUsageKind.field,
      'return_type' => TypeUsageKind.returnType,
      'inheritance' => TypeUsageKind.inheritance,
      'typedef_mention' => TypeUsageKind.typedefMention,
      _ => TypeUsageKind.variableDeclaration,
    };
  }

  /// Short label shown next to a usage's location in the usages panel.
  String get label {
    return switch (this) {
      TypeUsageKind.variableDeclaration => 'variable',
      TypeUsageKind.parameter => 'parameter',
      TypeUsageKind.field => 'field',
      TypeUsageKind.returnType => 'return type',
      TypeUsageKind.inheritance => 'base class',
      TypeUsageKind.typedefMention => 'typedef',
    };
  }
}

/// One occurrence of a project type being used at a specific source
/// location (US-4). Mirrors `TypeUsage` in
/// `crates/server/src/type_catalog.rs`.
class TypeUsage {
  const TypeUsage({
    required this.typeUsr,
    required this.kind,
    required this.file,
    required this.line,
    required this.column,
  });

  factory TypeUsage.fromJson(Map<String, Object?> json) {
    return TypeUsage(
      typeUsr: json['type_usr'] as String? ?? '',
      kind: TypeUsageKind.fromJson(json['kind'] as String?),
      file: json['file'] as String? ?? '',
      line: json['line'] as int? ?? 0,
      column: json['column'] as int? ?? 0,
    );
  }

  final String typeUsr;
  final TypeUsageKind kind;
  final String file;
  final int line;
  final int column;
}

/// The response of `GET /projects/types` (US-3/US-4): the type catalog plus
/// how many times each type is used, keyed by `usr`. Kept separate from
/// [TypeDeclaration] itself so the type-catalog model stays free of
/// usage-index concerns; the type list looks up each row's own [usr] to
/// render its count.
class TypeCatalogListing {
  const TypeCatalogListing({required this.types, required this.usageCounts});

  factory TypeCatalogListing.fromJson(Map<String, Object?> json) {
    final typesJson = json['types'] as List<Object?>? ?? const <Object?>[];
    final usageCountsJson =
        json['usage_counts'] as Map<String, Object?>? ??
        const <String, Object?>{};

    return TypeCatalogListing(
      types: typesJson
          .whereType<Map<String, Object?>>()
          .map(TypeDeclaration.fromJson)
          .toList(),
      usageCounts: usageCountsJson.map(
        (usr, count) => MapEntry(usr, count as int? ?? 0),
      ),
    );
  }

  final List<TypeDeclaration> types;
  final Map<String, int> usageCounts;
}

/// Mirrors `PointerDeclarationKind` in `crates/server/src/pointer_catalog.rs`:
/// where a declared pointer sits in the source.
enum PointerDeclarationKind {
  parameter,
  field,
  local,
  returnType;

  static PointerDeclarationKind fromJson(String? value) {
    return switch (value) {
      'parameter' => PointerDeclarationKind.parameter,
      'field' => PointerDeclarationKind.field,
      'local' => PointerDeclarationKind.local,
      'return_type' => PointerDeclarationKind.returnType,
      _ => PointerDeclarationKind.parameter,
    };
  }

  /// Short label shown next to a pointer's name in the catalog.
  String get label {
    return switch (this) {
      PointerDeclarationKind.parameter => 'parameter',
      PointerDeclarationKind.field => 'field',
      PointerDeclarationKind.local => 'local',
      PointerDeclarationKind.returnType => 'return type',
    };
  }
}

/// Mirrors `PointerShape` in `crates/server/src/pointer_catalog.rs`: what a
/// pointer points at, one level down.
enum PointerShape {
  scalar,
  doublePointer,
  functionPointer;

  static PointerShape fromJson(String? value) {
    return switch (value) {
      'scalar' => PointerShape.scalar,
      'double_pointer' => PointerShape.doublePointer,
      'function_pointer' => PointerShape.functionPointer,
      _ => PointerShape.scalar,
    };
  }

  /// Short label shown next to a pointer's pointee type in the catalog.
  String get label {
    return switch (this) {
      PointerShape.scalar => 'T*',
      PointerShape.doublePointer => 'T**',
      PointerShape.functionPointer => 'function pointer',
    };
  }
}

/// One raw C++ pointer declared in the project (Parte 1 of
/// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`). Mirrors
/// `PointerDeclaration` in `crates/server/src/pointer_catalog.rs`.
class PointerDeclaration {
  const PointerDeclaration({
    required this.kind,
    required this.shape,
    this.name = '',
    this.pointeeTypeName = '',
    this.pointeeUsr = '',
    required this.file,
    required this.line,
    required this.column,
    this.usr = '',
  });

  factory PointerDeclaration.fromJson(Map<String, Object?> json) {
    return PointerDeclaration(
      kind: PointerDeclarationKind.fromJson(json['kind'] as String?),
      shape: PointerShape.fromJson(json['shape'] as String?),
      name: json['name'] as String? ?? '',
      pointeeTypeName: json['pointee_type_name'] as String? ?? '',
      pointeeUsr: json['pointee_usr'] as String? ?? '',
      file: json['file'] as String? ?? '',
      line: json['line'] as int? ?? 0,
      column: json['column'] as int? ?? 0,
      usr: json['usr'] as String? ?? '',
    );
  }

  final PointerDeclarationKind kind;
  final PointerShape shape;

  /// For [PointerDeclarationKind.returnType], this is the *function's* own
  /// name — the pointer itself has none.
  final String name;
  final String pointeeTypeName;

  /// The pointee's own `usr` ([TypeDeclaration.usr]) when it's a named type
  /// the project's own type catalog already tracks; empty for `void`, a
  /// scalar, or a function type.
  final String pointeeUsr;
  final String file;
  final int line;
  final int column;
  final String usr;
}

/// Mirrors `FunctionDeclarationKind` in
/// `crates/server/src/function_catalog.rs`.
enum FunctionDeclarationKind {
  freeFunction,
  method,
  constructor,
  destructor,
  functionMacro,
  functionTemplate;

  static FunctionDeclarationKind fromJson(String? value) {
    return switch (value) {
      'free_function' => FunctionDeclarationKind.freeFunction,
      'method' => FunctionDeclarationKind.method,
      'constructor' => FunctionDeclarationKind.constructor,
      'destructor' => FunctionDeclarationKind.destructor,
      'function_macro' => FunctionDeclarationKind.functionMacro,
      'function_template' => FunctionDeclarationKind.functionTemplate,
      _ => FunctionDeclarationKind.freeFunction,
    };
  }

  /// Short label shown next to a callable's name in the catalog.
  String get label {
    return switch (this) {
      FunctionDeclarationKind.freeFunction => 'function',
      FunctionDeclarationKind.method => 'method',
      FunctionDeclarationKind.constructor => 'constructor',
      FunctionDeclarationKind.destructor => 'destructor',
      FunctionDeclarationKind.functionMacro => 'function macro',
      FunctionDeclarationKind.functionTemplate => 'template',
    };
  }
}

/// One callable declared in the project (US-5): a free function, method,
/// constructor, destructor, or function-like macro. Mirrors
/// `FunctionDeclaration` in `crates/server/src/function_catalog.rs`.
class FunctionDeclaration {
  const FunctionDeclaration({
    required this.name,
    required this.kind,
    this.namespace = '',
    this.owningClassUsr,
    required this.signature,
    required this.file,
    required this.line,
    required this.column,
    this.endLine = 0,
    this.endColumn = 0,
    this.usr = '',
    this.isVirtual = false,
    this.overriddenUsrs = const <String>[],
  });

  factory FunctionDeclaration.fromJson(Map<String, Object?> json) {
    return FunctionDeclaration(
      name: json['name'] as String? ?? '',
      kind: FunctionDeclarationKind.fromJson(json['kind'] as String?),
      namespace: json['namespace'] as String? ?? '',
      owningClassUsr: json['owning_class_usr'] as String?,
      signature: json['signature'] as String? ?? '',
      file: json['file'] as String? ?? '',
      line: json['line'] as int? ?? 0,
      column: json['column'] as int? ?? 0,
      endLine: json['end_line'] as int? ?? 0,
      endColumn: json['end_column'] as int? ?? 0,
      usr: json['usr'] as String? ?? '',
      isVirtual: json['is_virtual'] as bool? ?? false,
      overriddenUsrs:
          (json['overridden_usrs'] as List<dynamic>?)
              ?.map((entry) => entry as String)
              .toList() ??
          const <String>[],
    );
  }

  final String name;
  final FunctionDeclarationKind kind;
  final String namespace;

  /// `usr` of the owning struct/class/union, for a method/constructor/
  /// destructor — `null` for a free function or macro.
  final String? owningClassUsr;
  final String signature;
  final String file;
  final int line;
  final int column;
  final int endLine;
  final int endColumn;
  final String usr;
  final bool isVirtual;

  /// `usr` of every virtual method this one overrides — more than one under
  /// multiple inheritance, empty when it overrides nothing.
  final List<String> overriddenUsrs;

  /// The function's name, qualified with its enclosing namespace when it has
  /// one, so overloads and homonyms in different scopes read as distinct
  /// (US-5 criterion 2) — full disambiguation comes from [signature]. Shared
  /// by every UI surface that lists functions (`FunctionsView`,
  /// `CallersView`) rather than duplicated per file.
  String get qualifiedName => namespace.isEmpty ? name : '$namespace::$name';
}

/// Mirrors `CallResolution` in `crates/server/src/function_catalog.rs`:
/// whether a call site's target could be determined statically (US-5
/// criterion 6), and if so, whether that's only the statically-known target
/// of a virtual dispatch (criterion 3).
class CallResolution {
  const CallResolution.resolved({
    required this.calleeUsr,
    required this.isDynamicDispatch,
  }) : unresolvedReason = null;

  const CallResolution.unresolved({required this.unresolvedReason})
    : calleeUsr = null,
      isDynamicDispatch = false;

  factory CallResolution.fromJson(Map<String, Object?> json) {
    if (json['status'] == 'resolved') {
      return CallResolution.resolved(
        calleeUsr: json['callee_usr'] as String? ?? '',
        isDynamicDispatch: json['is_dynamic_dispatch'] as bool? ?? false,
      );
    }

    return CallResolution.unresolved(
      unresolvedReason: json['reason'] as String? ?? '',
    );
  }

  final String? calleeUsr;
  final bool isDynamicDispatch;
  final String? unresolvedReason;

  bool get isResolved => calleeUsr != null;
}

/// One call site in the static call graph (US-5). Mirrors `CallEdge` in
/// `crates/server/src/function_catalog.rs`.
class CallEdge {
  const CallEdge({
    required this.callerUsr,
    required this.resolution,
    required this.file,
    required this.line,
    required this.column,
  });

  factory CallEdge.fromJson(Map<String, Object?> json) {
    return CallEdge(
      callerUsr: json['caller_usr'] as String? ?? '',
      resolution: CallResolution.fromJson(
        json['resolution'] as Map<String, Object?>? ?? const {},
      ),
      file: json['file'] as String? ?? '',
      line: json['line'] as int? ?? 0,
      column: json['column'] as int? ?? 0,
    );
  }

  final String callerUsr;
  final CallResolution resolution;
  final String file;
  final int line;
  final int column;
}

/// The response of `GET /projects/functions` (US-5): the function catalog
/// plus how many recorded callers each one has, keyed by `usr`. Mirrors
/// `TypeCatalogListing`'s split between declarations and a usage/caller
/// index.
class FunctionCatalogListing {
  const FunctionCatalogListing({
    required this.functions,
    required this.callerCounts,
  });

  factory FunctionCatalogListing.fromJson(Map<String, Object?> json) {
    final functionsJson =
        json['functions'] as List<Object?>? ?? const <Object?>[];
    final callerCountsJson =
        json['caller_counts'] as Map<String, Object?>? ??
        const <String, Object?>{};

    return FunctionCatalogListing(
      functions: functionsJson
          .whereType<Map<String, Object?>>()
          .map(FunctionDeclaration.fromJson)
          .toList(),
      callerCounts: callerCountsJson.map(
        (usr, count) => MapEntry(usr, count as int? ?? 0),
      ),
    );
  }

  final List<FunctionDeclaration> functions;
  final Map<String, int> callerCounts;
}

/// How far a single `libclang` extraction pass (type cataloging or source
/// file discovery) has gotten. Mirrors `ExtractionProgress` in
/// `crates/server/src/progress.rs`; `total == 0` means that pass hasn't
/// started yet.
class ExtractionProgress {
  const ExtractionProgress({required this.completed, required this.total});

  factory ExtractionProgress.fromJson(Map<String, Object?> json) {
    return ExtractionProgress(
      completed: json['completed'] as int? ?? 0,
      total: json['total'] as int? ?? 0,
    );
  }

  final int completed;
  final int total;
}

/// Mirrors `JobPhase` in `crates/server/src/jobs.rs`: where a still-running
/// project creation job stands.
enum ProjectCreationJobPhase {
  ingesting,
  catalogingTypes,
  discoveringSourceFiles,
  catalogingFunctions,
  persisting;

  static ProjectCreationJobPhase fromJson(String? value) {
    return switch (value) {
      'ingesting' => ProjectCreationJobPhase.ingesting,
      'cataloging_types' => ProjectCreationJobPhase.catalogingTypes,
      'discovering_source_files' =>
        ProjectCreationJobPhase.discoveringSourceFiles,
      'cataloging_functions' => ProjectCreationJobPhase.catalogingFunctions,
      'persisting' => ProjectCreationJobPhase.persisting,
      _ => ProjectCreationJobPhase.ingesting,
    };
  }

  /// Short label shown as a log line when entering this phase.
  String get label {
    return switch (this) {
      ProjectCreationJobPhase.ingesting => 'Extracting project files',
      ProjectCreationJobPhase.catalogingTypes => 'Cataloging types',
      ProjectCreationJobPhase.discoveringSourceFiles =>
        'Discovering source files',
      ProjectCreationJobPhase.catalogingFunctions =>
        'Cataloging functions and calls',
      ProjectCreationJobPhase.persisting => 'Saving project',
    };
  }
}

/// `cancelling` and `cancelled` are US-4 criterion 7: a `DELETE
/// /projects/jobs/{id}` request was made, but stopping the background
/// `libclang` pass is best-effort, so there's a window where the job is
/// still `running` from the pipeline's own point of view while already
/// `cancelling` from the user's — distinct from `cancelled`, the terminal
/// outcome once the pipeline actually noticed and stopped.
enum ProjectCreationJobState {
  running,
  cancelling,
  succeeded,
  failed,
  cancelled,
}

/// A snapshot of `GET /projects/jobs/{id}`: whether the background project
/// creation job (started by `startCreateProject`) is still running, and if
/// so how far along each `libclang` pass is; or its terminal outcome.
class ProjectCreationJobStatus {
  const ProjectCreationJobStatus({
    required this.state,
    this.phase,
    this.typeCatalogProgress,
    this.sourceCatalogProgress,
    this.functionCatalogProgress,
    this.project,
    this.errorMessage,
    this.isClientError = false,
  });

  factory ProjectCreationJobStatus.fromJson(Map<String, Object?> json) {
    final state = switch (json['status'] as String?) {
      'cancelling' => ProjectCreationJobState.cancelling,
      'succeeded' => ProjectCreationJobState.succeeded,
      'failed' => ProjectCreationJobState.failed,
      'cancelled' => ProjectCreationJobState.cancelled,
      _ => ProjectCreationJobState.running,
    };
    // The server includes `phase`/progress alongside both `"running"` and
    // `"cancelling"` — they're the same not-yet-finished response, differing
    // only in whether a cancel was requested.
    final stillRunning =
        state == ProjectCreationJobState.running ||
        state == ProjectCreationJobState.cancelling;

    final typeCatalogJson = json['type_catalog'] as Map<String, Object?>?;
    final sourceCatalogJson = json['source_catalog'] as Map<String, Object?>?;
    final functionCatalogJson =
        json['function_catalog'] as Map<String, Object?>?;
    final projectJson = json['project'] as Map<String, Object?>?;

    return ProjectCreationJobStatus(
      state: state,
      phase: stillRunning
          ? ProjectCreationJobPhase.fromJson(json['phase'] as String?)
          : null,
      typeCatalogProgress: typeCatalogJson != null
          ? ExtractionProgress.fromJson(typeCatalogJson)
          : null,
      sourceCatalogProgress: sourceCatalogJson != null
          ? ExtractionProgress.fromJson(sourceCatalogJson)
          : null,
      functionCatalogProgress: functionCatalogJson != null
          ? ExtractionProgress.fromJson(functionCatalogJson)
          : null,
      project: projectJson != null
          ? CreatedProject.fromJson(projectJson)
          : null,
      errorMessage: json['message'] as String?,
      isClientError: json['is_client_error'] as bool? ?? false,
    );
  }

  final ProjectCreationJobState state;
  final ProjectCreationJobPhase? phase;
  final ExtractionProgress? typeCatalogProgress;
  final ExtractionProgress? sourceCatalogProgress;
  final ExtractionProgress? functionCatalogProgress;
  final CreatedProject? project;
  final String? errorMessage;
  final bool isClientError;
}

/// The response of `POST /projects/transpile` (US-8, E01–E03 scope): the
/// Dart package emitted from the project's free functions and `struct`s.
/// Mirrors `transpile::TranspiledPackage` in `crates/server/src/transpile.rs`.
class TranspiledPackage {
  const TranspiledPackage({required this.packageName, required this.files});

  factory TranspiledPackage.fromJson(Map<String, Object?> json) {
    final filesJson = json['files'] as Map<String, Object?>? ?? const {};
    return TranspiledPackage(
      packageName: json['package_name'] as String? ?? '',
      files: filesJson.map(
        (path, content) => MapEntry(path, content as String? ?? ''),
      ),
    );
  }

  final String packageName;

  /// Package-relative path (`"pubspec.yaml"`, `"lib/aritmetica.dart"`) →
  /// file contents.
  final Map<String, String> files;
}

/// Where a piece of generated Dart came from in the original C++. Mirrors
/// `ir::Origin` in `crates/server/src/ir/mod.rs`.
class CppOrigin {
  const CppOrigin({
    required this.file,
    required this.line,
    required this.column,
  });

  factory CppOrigin.fromJson(Map<String, Object?> json) {
    return CppOrigin(
      file: json['file'] as String? ?? '',
      line: (json['line'] as num?)?.toInt() ?? 0,
      column: (json['column'] as num?)?.toInt() ?? 0,
    );
  }

  final String file;
  final int line;
  final int column;
}

/// Severity of a `dart analyze` finding. Mirrors
/// `validate::dart::Severity` in `crates/server/src/validate/dart.rs`.
enum DartDiagnosticSeverity {
  error,
  warning,
  info;

  static DartDiagnosticSeverity fromJson(String? value) {
    switch (value) {
      case 'error':
        return DartDiagnosticSeverity.error;
      case 'warning':
        return DartDiagnosticSeverity.warning;
      default:
        return DartDiagnosticSeverity.info;
    }
  }
}

/// One `dart analyze` finding against the transpiled package (US-9), with
/// its C++ origin when one could be located. Mirrors
/// `validate::dart::DartDiagnostic` in `crates/server/src/validate/dart.rs`
/// — see that type's doc comment for the granularity `origin` resolves at
/// (a whole top-level declaration, not the exact statement).
class DartDiagnostic {
  const DartDiagnostic({
    required this.severity,
    required this.message,
    required this.dartFile,
    required this.dartLine,
    required this.origin,
  });

  factory DartDiagnostic.fromJson(Map<String, Object?> json) {
    final originJson = json['origin'] as Map<String, Object?>?;
    return DartDiagnostic(
      severity: DartDiagnosticSeverity.fromJson(json['severity'] as String?),
      message: json['message'] as String? ?? '',
      dartFile: json['dart_file'] as String? ?? '',
      dartLine: (json['dart_line'] as num?)?.toInt() ?? 0,
      origin: originJson == null ? null : CppOrigin.fromJson(originJson),
    );
  }

  final DartDiagnosticSeverity severity;
  final String message;
  final String dartFile;
  final int dartLine;
  final CppOrigin? origin;
}

/// Why a usr is (or would be) external. Mirrors `externals::ExternalSource`
/// in `crates/server/src/externals.rs` — see
/// `docs/plans/lista-de-externos.md`. `pattern` is only set for
/// [kind] `nameRegex`/`pathRegex`.
enum ExternalSourceKind {
  manualInclude,
  manualExclude,
  nameRegex,
  pathRegex,
  autoUndefinedFunction;

  static ExternalSourceKind fromJson(String? value) {
    return switch (value) {
      'manual_include' => ExternalSourceKind.manualInclude,
      'manual_exclude' => ExternalSourceKind.manualExclude,
      'name_regex' => ExternalSourceKind.nameRegex,
      'path_regex' => ExternalSourceKind.pathRegex,
      'auto_undefined_function' => ExternalSourceKind.autoUndefinedFunction,
      _ => ExternalSourceKind.autoUndefinedFunction,
    };
  }

  /// Short label for the source badge shown next to an item in the
  /// Extern view.
  String get label {
    return switch (this) {
      ExternalSourceKind.manualInclude => 'manual',
      ExternalSourceKind.manualExclude => 'excluído manualmente',
      ExternalSourceKind.nameRegex => 'regexp de nome',
      ExternalSourceKind.pathRegex => 'regexp de caminho',
      ExternalSourceKind.autoUndefinedFunction => 'detectado automaticamente',
    };
  }
}

class ExternalSource {
  const ExternalSource({required this.kind, this.pattern});

  factory ExternalSource.fromJson(Map<String, Object?> json) {
    return ExternalSource(
      kind: ExternalSourceKind.fromJson(json['kind'] as String?),
      pattern: json['pattern'] as String?,
    );
  }

  final ExternalSourceKind kind;
  final String? pattern;
}

/// One usr's full external status — whether it's currently in the effective
/// external set, and every source that contributed to that (manual mark,
/// either regexp list, auto-detection). Mirrors `externals::ExternalStatus`.
class ExternalStatus {
  const ExternalStatus({
    required this.usr,
    required this.effective,
    required this.sources,
  });

  factory ExternalStatus.fromJson(Map<String, Object?> json) {
    final sourcesJson = json['sources'] as List<Object?>? ?? const <Object?>[];
    return ExternalStatus(
      usr: json['usr'] as String? ?? '',
      effective: json['effective'] as bool? ?? false,
      sources: sourcesJson
          .whereType<Map<String, Object?>>()
          .map(ExternalSource.fromJson)
          .toList(),
    );
  }

  final String usr;
  final bool effective;
  final List<ExternalSource> sources;
}

/// One name-regexp rule (decision 6, `docs/plans/lista-de-externos.md`) —
/// matches a qualified C++ name. Mirrors `externals::NameRegexRule`.
class NameRegexRule {
  const NameRegexRule({
    required this.id,
    required this.pattern,
    required this.createdAt,
  });

  factory NameRegexRule.fromJson(Map<String, Object?> json) {
    return NameRegexRule(
      id: (json['id'] as num?)?.toInt() ?? 0,
      pattern: json['pattern'] as String? ?? '',
      createdAt: json['created_at'] as String? ?? '',
    );
  }

  final int id;
  final String pattern;
  final String createdAt;
}

/// One path-regexp rule — matches a declaration's file path. Mirrors
/// `externals::PathRegexRule`.
class PathRegexRule {
  const PathRegexRule({
    required this.id,
    required this.pattern,
    required this.createdAt,
  });

  factory PathRegexRule.fromJson(Map<String, Object?> json) {
    return PathRegexRule(
      id: (json['id'] as num?)?.toInt() ?? 0,
      pattern: json['pattern'] as String? ?? '',
      createdAt: json['created_at'] as String? ?? '',
    );
  }

  final int id;
  final String pattern;
  final String createdAt;
}

/// The response of `GET /projects/externals`: the effective external set
/// plus both live regexp rule lists. Mirrors
/// `project_service::ExternalListing`.
class ExternalListing {
  const ExternalListing({
    required this.statuses,
    required this.nameRegexes,
    required this.pathRegexes,
  });

  factory ExternalListing.fromJson(Map<String, Object?> json) {
    final statusesJson =
        json['statuses'] as List<Object?>? ?? const <Object?>[];
    final nameRegexesJson =
        json['name_regexes'] as List<Object?>? ?? const <Object?>[];
    final pathRegexesJson =
        json['path_regexes'] as List<Object?>? ?? const <Object?>[];

    return ExternalListing(
      statuses: statusesJson
          .whereType<Map<String, Object?>>()
          .map(ExternalStatus.fromJson)
          .toList(),
      nameRegexes: nameRegexesJson
          .whereType<Map<String, Object?>>()
          .map(NameRegexRule.fromJson)
          .toList(),
      pathRegexes: pathRegexesJson
          .whereType<Map<String, Object?>>()
          .map(PathRegexRule.fromJson)
          .toList(),
    );
  }

  final List<ExternalStatus> statuses;
  final List<NameRegexRule> nameRegexes;
  final List<PathRegexRule> pathRegexes;
}
