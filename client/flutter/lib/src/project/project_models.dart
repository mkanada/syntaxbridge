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
  persisting;

  static ProjectCreationJobPhase fromJson(String? value) {
    return switch (value) {
      'ingesting' => ProjectCreationJobPhase.ingesting,
      'cataloging_types' => ProjectCreationJobPhase.catalogingTypes,
      'discovering_source_files' =>
        ProjectCreationJobPhase.discoveringSourceFiles,
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
  final CreatedProject? project;
  final String? errorMessage;
  final bool isClientError;
}
