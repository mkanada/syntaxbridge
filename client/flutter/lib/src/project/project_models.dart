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
    required this.buildLayers,
    required this.buildDependencyLayers,
    required this.compilationUnits,
  });

  factory CreatedProject.fromJson(Map<String, Object?> json) {
    final unitsJson =
        json['compilation_units'] as List<Object?>? ?? const <Object?>[];
    final layersJson =
        json['build_layers'] as List<Object?>? ?? const <Object?>[];
    final dependencyLayersJson =
        json['build_dependency_layers'] as List<Object?>? ?? const <Object?>[];

    return CreatedProject(
      name: json['name'] as String? ?? 'unknown',
      projectDir: json['project_dir'] as String? ?? '',
      inputSourceDir: json['input_source_dir'] as String? ?? '',
      buildLayers: layersJson
          .whereType<Map<String, Object?>>()
          .map(BuildLayer.fromJson)
          .toList(),
      buildDependencyLayers: dependencyLayersJson
          .whereType<Map<String, Object?>>()
          .map(BuildDependencyLayer.fromJson)
          .toList(),
      compilationUnits: unitsJson
          .whereType<Map<String, Object?>>()
          .map(CompilationUnit.fromJson)
          .toList(),
    );
  }

  final String name;
  final String projectDir;
  final String inputSourceDir;
  final List<BuildLayer> buildLayers;
  final List<BuildDependencyLayer> buildDependencyLayers;
  final List<CompilationUnit> compilationUnits;
}

class BuildLayer {
  const BuildLayer({required this.index, required this.targets});

  factory BuildLayer.fromJson(Map<String, Object?> json) {
    final targetsJson = json['targets'] as List<Object?>? ?? const [];

    return BuildLayer(
      index: json['index'] as int? ?? 0,
      targets: targetsJson
          .whereType<Map<String, Object?>>()
          .map(BuildTarget.fromJson)
          .toList(),
    );
  }

  final int index;
  final List<BuildTarget> targets;
}

class BuildTarget {
  const BuildTarget({required this.id, required this.name, required this.kind});

  factory BuildTarget.fromJson(Map<String, Object?> json) {
    return BuildTarget(
      id: json['id'] as String? ?? '',
      name: json['name'] as String? ?? 'unknown',
      kind: json['kind'] as String? ?? 'UNKNOWN',
    );
  }

  final String id;
  final String name;
  final String kind;
}

class BuildDependencyLayer {
  const BuildDependencyLayer({required this.index, required this.items});

  factory BuildDependencyLayer.fromJson(Map<String, Object?> json) {
    final itemsJson = json['items'] as List<Object?>? ?? const [];

    return BuildDependencyLayer(
      index: json['index'] as int? ?? 0,
      items: itemsJson
          .whereType<Map<String, Object?>>()
          .map(BuildDependencyItem.fromJson)
          .toList(),
    );
  }

  final int index;
  final List<BuildDependencyItem> items;
}

class BuildDependencyItem {
  const BuildDependencyItem({
    required this.id,
    required this.name,
    required this.kind,
    required this.dependencies,
  });

  factory BuildDependencyItem.fromJson(Map<String, Object?> json) {
    final dependenciesJson = json['dependencies'] as List<Object?>? ?? const [];

    return BuildDependencyItem(
      id: json['id'] as String? ?? '',
      name: json['name'] as String? ?? 'unknown',
      kind: json['kind'] as String? ?? 'UNKNOWN',
      dependencies: dependenciesJson.whereType<String>().toList(),
    );
  }

  final String id;
  final String name;
  final String kind;
  final List<String> dependencies;
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
