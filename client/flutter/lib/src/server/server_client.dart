import '../project/project_models.dart';

abstract class ServerClient {
  Future<ServerStatus> health();

  Future<CreatedProject> createProject(CreateProjectInput input);
}
