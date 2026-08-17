import 'package:syntax_bridge/main.dart';

/// A scriptable [ServerClient] shared by the screenshot test suite, so each
/// screen only has to say what data it needs instead of re-implementing the
/// whole interface. Mirrors `_FakeServerClient` in `test/app_test.dart`.
class ScreenshotFakeServerClient implements ServerClient {
  ScreenshotFakeServerClient(
    this.status, {
    this.project,
    this.createError,
    this.sourceFileContent = '',
    this.recentProjects = const <RecentProject>[],
    this.openProjectResult,
    this.openProjectError,
    this.types = const <TypeDeclaration>[],
    this.usageCounts = const <String, int>{},
    this.usagesByType = const <String, List<TypeUsage>>{},
    this.pointers = const <PointerDeclaration>[],
    this.functions = const <FunctionDeclaration>[],
    this.callerCounts = const <String, int>{},
    this.callersByFunction = const <String, List<CallEdge>>{},
    this.callsByFile = const <String, List<CallEdge>>{},
    this.transpiledPackage,
    this.diagnostics = const <DartDiagnostic>[],
    ExternalListing externalListing = const ExternalListing(
      statuses: <ExternalStatus>[],
      nameRegexes: <NameRegexRule>[],
      pathRegexes: <PathRegexRule>[],
    ),
    this.fileMarkResult = const <String>[],
    this.typeMarkResult = const <String>[],
    // ignore: prefer_initializing_formals
  }) : _externalListing = externalListing;

  final ServerStatus status;
  final CreatedProject? project;
  final Object? createError;
  final String sourceFileContent;
  final List<RecentProject> recentProjects;
  final CreatedProject? openProjectResult;
  final Object? openProjectError;
  final List<TypeDeclaration> types;
  final Map<String, int> usageCounts;
  final Map<String, List<TypeUsage>> usagesByType;
  final List<PointerDeclaration> pointers;
  final List<FunctionDeclaration> functions;
  final Map<String, int> callerCounts;
  final Map<String, List<CallEdge>> callersByFunction;
  final Map<String, List<CallEdge>> callsByFile;
  final TranspiledPackage? transpiledPackage;
  final List<DartDiagnostic> diagnostics;

  /// Returned by [markFileExternal]/[markTypeExternal] — the fake doesn't
  /// replicate the server's `expand_file_mark`/`expand_type_mark`
  /// expansion, so a screenshot test that needs to show the *result* of
  /// marking a file/type should construct [ExternalListing] with that
  /// result already reflected, rather than relying on these calls to
  /// mutate it.
  final List<String> fileMarkResult;
  final List<String> typeMarkResult;
  ExternalListing _externalListing;
  String? markedFile;
  String? markedTypeUsr;
  String? createdProjectName;
  String? readSourceFilePath;
  String? openedProjectDir;
  String? forgottenProjectDir;
  String? transpileProjectDir;
  String? validateProjectDir;
  late List<RecentProject> _remainingProjects = recentProjects;

  @override
  Future<ServerStatus> health() async => status;

  @override
  Future<void> forgetProject(String projectDir) async {
    forgottenProjectDir = projectDir;
    _remainingProjects = _remainingProjects
        .where((project) => project.projectDir != projectDir)
        .toList();
  }

  @override
  Future<String> startCreateProject(CreateProjectInput input) async {
    createdProjectName = input.name;
    return 'job-1';
  }

  @override
  Future<void> cancelCreateProject(String jobId) async {}

  @override
  Future<ProjectCreationJobStatus> pollCreateProjectJob(String jobId) async {
    final error = createError;
    if (error != null) {
      final message = error is ProjectCreationException
          ? error.message
          : error.toString();
      return ProjectCreationJobStatus(
        state: ProjectCreationJobState.failed,
        errorMessage: message,
        isClientError: true,
      );
    }

    return ProjectCreationJobStatus(
      state: ProjectCreationJobState.succeeded,
      project:
          project ??
          const CreatedProject(
            name: 'empty',
            projectDir: '',
            inputSourceDir: '',
            compilationUnits: [],
          ),
    );
  }

  @override
  Future<List<RecentProject>> listRecentProjects() async => _remainingProjects;

  @override
  Future<CreatedProject> openProject(String projectDir) async {
    openedProjectDir = projectDir;
    final error = openProjectError;
    if (error != null) {
      throw error;
    }

    return openProjectResult ??
        const CreatedProject(
          name: 'opened',
          projectDir: '',
          inputSourceDir: '',
          compilationUnits: [],
        );
  }

  @override
  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  }) async {
    readSourceFilePath = path;
    return sourceFileContent;
  }

  @override
  Future<TypeCatalogListing> listTypes(String projectDir) async =>
      TypeCatalogListing(types: types, usageCounts: usageCounts);

  @override
  Future<List<TypeUsage>> listTypeUsages({
    required String projectDir,
    required String typeUsr,
  }) async => usagesByType[typeUsr] ?? const <TypeUsage>[];

  @override
  Future<List<PointerDeclaration>> listPointers(String projectDir) async =>
      pointers;

  @override
  Future<FunctionCatalogListing> listFunctions(String projectDir) async =>
      FunctionCatalogListing(functions: functions, callerCounts: callerCounts);

  @override
  Future<List<CallEdge>> listCallers({
    required String projectDir,
    required String functionUsr,
  }) async => callersByFunction[functionUsr] ?? const <CallEdge>[];

  @override
  Future<List<CallEdge>> listCallsInFile({
    required String projectDir,
    required String file,
  }) async => callsByFile[file] ?? const <CallEdge>[];

  @override
  Future<TranspiledPackage> transpileProject(String projectDir) async {
    transpileProjectDir = projectDir;
    return transpiledPackage ??
        const TranspiledPackage(packageName: 'output', files: {});
  }

  @override
  Future<List<DartDiagnostic>> validateProject(String projectDir) async {
    validateProjectDir = projectDir;
    return diagnostics;
  }

  @override
  Future<ExternalListing> listExternals(String projectDir) async =>
      _externalListing;

  @override
  Future<void> markExternal({
    required String projectDir,
    required String usr,
    required bool external,
  }) async {
    final withoutUsr = _externalListing.statuses
        .where((status) => status.usr != usr)
        .toList();
    _externalListing = ExternalListing(
      statuses: [
        ...withoutUsr,
        ExternalStatus(
          usr: usr,
          effective: external,
          sources: [
            ExternalSource(
              kind: external
                  ? ExternalSourceKind.manualInclude
                  : ExternalSourceKind.manualExclude,
            ),
          ],
        ),
      ],
      nameRegexes: _externalListing.nameRegexes,
      pathRegexes: _externalListing.pathRegexes,
    );
  }

  @override
  Future<List<String>> markFileExternal({
    required String projectDir,
    required String file,
  }) async {
    markedFile = file;
    return fileMarkResult;
  }

  @override
  Future<List<String>> markTypeExternal({
    required String projectDir,
    required String typeUsr,
  }) async {
    markedTypeUsr = typeUsr;
    return typeMarkResult;
  }

  @override
  Future<NameRegexRule> addNameRegex({
    required String projectDir,
    required String pattern,
  }) async {
    final rule = NameRegexRule(
      id: _externalListing.nameRegexes.length + 1,
      pattern: pattern,
      createdAt: '0',
    );
    _externalListing = ExternalListing(
      statuses: _externalListing.statuses,
      nameRegexes: [..._externalListing.nameRegexes, rule],
      pathRegexes: _externalListing.pathRegexes,
    );
    return rule;
  }

  @override
  Future<void> removeNameRegex({
    required String projectDir,
    required int id,
  }) async {
    _externalListing = ExternalListing(
      statuses: _externalListing.statuses,
      nameRegexes: _externalListing.nameRegexes
          .where((rule) => rule.id != id)
          .toList(),
      pathRegexes: _externalListing.pathRegexes,
    );
  }

  @override
  Future<PathRegexRule> addPathRegex({
    required String projectDir,
    required String pattern,
  }) async {
    final rule = PathRegexRule(
      id: _externalListing.pathRegexes.length + 1,
      pattern: pattern,
      createdAt: '0',
    );
    _externalListing = ExternalListing(
      statuses: _externalListing.statuses,
      nameRegexes: _externalListing.nameRegexes,
      pathRegexes: [..._externalListing.pathRegexes, rule],
    );
    return rule;
  }

  @override
  Future<void> removePathRegex({
    required String projectDir,
    required int id,
  }) async {
    _externalListing = ExternalListing(
      statuses: _externalListing.statuses,
      nameRegexes: _externalListing.nameRegexes,
      pathRegexes: _externalListing.pathRegexes
          .where((rule) => rule.id != id)
          .toList(),
    );
  }
}

/// A scriptable [PathPicker] shared by the screenshot test suite. Mirrors
/// `_FakePathPicker` in `test/app_test.dart`.
class ScreenshotFakePathPicker implements PathPicker {
  ScreenshotFakePathPicker({
    required this.workspaceDir,
    required this.sourceArchive,
    this.existingProjectDir,
  });

  final String workspaceDir;
  final String sourceArchive;
  final String? existingProjectDir;
  int workspacePickCount = 0;
  int archivePickCount = 0;
  int existingProjectPickCount = 0;

  @override
  Future<String?> pickSourceArchive() async {
    archivePickCount += 1;
    return sourceArchive;
  }

  @override
  Future<String?> pickWorkspaceDirectory() async {
    workspacePickCount += 1;
    return workspaceDir;
  }

  @override
  Future<String?> pickExistingProjectDirectory() async {
    existingProjectPickCount += 1;
    return existingProjectDir;
  }
}
