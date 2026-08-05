import '../project/project_models.dart';

abstract class ServerClient {
  Future<ServerStatus> health();

  Future<CreatedProject> createProject(CreateProjectInput input);

  /// The last 5 projects the app was used with, most recently opened first.
  Future<List<RecentProject>> listRecentProjects();

  /// Reloads a project directly from its own persisted data, without
  /// running ingest again. Used both to reopen a recent project and to
  /// import a project that already exists on disk from a prior ingest.
  Future<CreatedProject> openProject(String projectDir);

  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  });
}
