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

  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  });

  /// The type catalog already persisted for a project (US-3), without
  /// reparsing it.
  Future<List<TypeDeclaration>> listTypes(String projectDir);
}
