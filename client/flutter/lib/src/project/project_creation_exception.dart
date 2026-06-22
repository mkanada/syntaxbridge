class ProjectCreationException implements Exception {
  const ProjectCreationException(this.message);

  final String message;

  @override
  String toString() => message;
}
