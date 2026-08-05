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
  });

  factory RecentProject.fromJson(Map<String, Object?> json) {
    return RecentProject(
      name: json['name'] as String? ?? 'unknown',
      projectDir: json['project_dir'] as String? ?? '',
      sourceLanguage: json['source_language'] as String? ?? '',
      targetLanguage: json['target_language'] as String? ?? '',
      lastIngestStatus: json['last_ingest_status'] as String? ?? '',
    );
  }

  final String name;
  final String projectDir;
  final String sourceLanguage;
  final String targetLanguage;
  final String lastIngestStatus;
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
