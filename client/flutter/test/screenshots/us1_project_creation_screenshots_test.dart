import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for US-1 (`docs/plans/User Steps.md`): every step of project
/// creation/ingestion a user interacts with, not just the final screen.
void main() {
  testWidgets('captures the landing page with no recent projects', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: ScreenshotFakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us1-landing-empty');
  });

  testWidgets('captures the landing page with recent projects', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: ScreenshotFakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
            recentProjects: const [
              RecentProject(
                name: 'counter',
                projectDir: '/tmp/projects/counter',
                sourceLanguage: 'cpp',
                targetLanguage: 'dart',
                lastIngestStatus: 'success',
              ),
              RecentProject(
                name: 'gone',
                projectDir: '/tmp/projects/gone',
                sourceLanguage: 'cpp',
                targetLanguage: 'dart',
                lastIngestStatus: 'success',
                available: false,
              ),
            ],
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us1-landing-recent-projects');
  });

  testWidgets('captures the empty new-project form', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: ScreenshotFakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'New project'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us1-new-project-form');
  });

  testWidgets('captures the import-project dialog', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);
    final pathPicker = ScreenshotFakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
      existingProjectDir: '/home/user/syntax-bridge-projects/verovio',
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: ScreenshotFakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
          ),
          pathPicker: pathPicker,
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(OutlinedButton, 'Import project'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us1-import-project-dialog');
  });

  testWidgets('captures the import-project dialog after a failed import', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);
    final pathPicker = ScreenshotFakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
      existingProjectDir: '/tmp/not-a-project',
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: ScreenshotFakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
            openProjectError: const ProjectCreationException(
              'no syntax-bridge project found at /tmp/not-a-project',
            ),
          ),
          pathPicker: pathPicker,
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(OutlinedButton, 'Import project'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Choose project directory'));
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Import'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us1-import-project-dialog-error');
  });

  testWidgets('captures the creation progress screen mid-ingestion', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);
    final client = _ScriptedServerClient(
      jobId: 'job-1',
      statuses: [
        const ProjectCreationJobStatus(
          state: ProjectCreationJobState.running,
          phase: ProjectCreationJobPhase.catalogingTypes,
          typeCatalogProgress: ExtractionProgress(completed: 5, total: 10),
          sourceCatalogProgress: ExtractionProgress(completed: 0, total: 0),
        ),
      ],
    );

    await tester.pumpWidget(
      MaterialApp(
        home: RepaintBoundary(
          key: captureKey,
          child: CreatingProjectPage(
            serverClient: client,
            input: const CreateProjectInput(
              name: 'counter',
              workspaceDir: '/tmp/projects',
              archivePath: '/tmp/source.tar.gz',
            ),
            pollInterval: const Duration(milliseconds: 10),
            onProjectCreated: (_) {},
            onCancel: () {},
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 10));

    await _capture(tester, captureKey, 'us1-project-creating-progress');
  });

  testWidgets('captures the creation failure screen with server details', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);
    final client = _ScriptedServerClient(
      jobId: 'job-1',
      statuses: [
        const ProjectCreationJobStatus(
          state: ProjectCreationJobState.failed,
          errorMessage: 'no CMakeLists.txt found under input-source',
          isClientError: true,
        ),
      ],
    );

    await tester.pumpWidget(
      MaterialApp(
        home: RepaintBoundary(
          key: captureKey,
          child: CreatingProjectPage(
            serverClient: client,
            input: const CreateProjectInput(
              name: 'counter',
              workspaceDir: '/tmp/projects',
              archivePath: '/tmp/source.tar.gz',
            ),
            pollInterval: const Duration(milliseconds: 10),
            onProjectCreated: (_) {},
            onCancel: () {},
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 10));

    await _capture(tester, captureKey, 'us1-project-creation-failed');
  });

  testWidgets('captures the connected project screen', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: ScreenshotFakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
            project: _projectWithCompilationUnits,
          ),
        ),
      ),
    );
    await _createProject(tester);

    await _capture(tester, captureKey, 'us1-project-created');
  });
}

Future<void> _createProject(WidgetTester tester) async {
  await tester.pumpAndSettle();
  await tester.tap(find.widgetWithText(FilledButton, 'New project'));
  await tester.pumpAndSettle();

  await tester.enterText(find.bySemanticsLabel('Project name'), 'counter');
  await tester.enterText(
    find.bySemanticsLabel('Workspace directory'),
    '/tmp/projects',
  );
  await tester.enterText(
    find.bySemanticsLabel('Source archive'),
    '/tmp/source.tar.gz',
  );
  await tester.pump();
  await tester.tap(find.widgetWithText(FilledButton, 'Create project'));
  await tester.pumpAndSettle();
}

Future<void> _capture(WidgetTester tester, GlobalKey key, String name) async {
  final artifact = await captureTestScreen(
    tester,
    name: name,
    boundaryKey: key,
  );
  expect(artifact.existsSync(), isTrue);
  expect(artifact.lengthSync(), greaterThan(0));
}

void _configureDesktopSurface(WidgetTester tester) {
  tester.view
    ..devicePixelRatio = 1
    ..physicalSize = const Size(1280, 900);
  addTearDown(tester.view.reset);
}

const _projectWithCompilationUnits = CreatedProject(
  name: 'counter',
  projectDir: '/tmp/projects/counter',
  inputSourceDir: '/tmp/projects/counter/input-source',
  compilationUnits: [
    CompilationUnit(
      directory: '/tmp/projects/counter/build',
      file: '/tmp/projects/counter/input-source/fixture/main.cpp',
      command: 'clang++ -c main.cpp',
    ),
  ],
);

/// A [ServerClient] whose `startCreateProject` always succeeds with [jobId],
/// and whose `pollCreateProjectJob` returns [statuses] in order (one per
/// call), holding on the last entry once exhausted. Mirrors
/// `_ScriptedServerClient` in `test/creating_project_page_test.dart`.
class _ScriptedServerClient implements ServerClient {
  _ScriptedServerClient({required this.jobId, required this.statuses});

  final String jobId;
  final List<ProjectCreationJobStatus> statuses;
  int _pollCount = 0;

  @override
  Future<String> startCreateProject(CreateProjectInput input) async => jobId;

  @override
  Future<ProjectCreationJobStatus> pollCreateProjectJob(String jobId) async {
    if (statuses.isEmpty) {
      return const ProjectCreationJobStatus(
        state: ProjectCreationJobState.running,
        phase: ProjectCreationJobPhase.ingesting,
      );
    }

    final index = _pollCount < statuses.length
        ? _pollCount
        : statuses.length - 1;
    _pollCount++;
    return statuses[index];
  }

  @override
  Future<void> cancelCreateProject(String jobId) async {}

  @override
  Future<ServerStatus> health() =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<List<RecentProject>> listRecentProjects() =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<void> forgetProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<CreatedProject> openProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<String> startAnalyseProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<AnalysisJobStatus> pollAnalyseProjectJob(String jobId) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<String> readSourceFile({
    required String projectDir,
    required String path,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<TypeCatalogListing> listTypes(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<List<PointerDeclaration>> listPointers(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<List<TypeUsage>> listTypeUsages({
    required String projectDir,
    required String typeUsr,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<FunctionCatalogListing> listFunctions(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<List<CallEdge>> listCallers({
    required String projectDir,
    required String functionUsr,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<List<CallEdge>> listCallsInFile({
    required String projectDir,
    required String file,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<TranspiledPackage> transpileProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<List<DartDiagnostic>> validateProject(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<ExternalListing> listExternals(String projectDir) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<void> markExternal({
    required String projectDir,
    required String usr,
    required bool external,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<void> markFileExternal({
    required String projectDir,
    required String file,
    required bool external,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<List<String>> markTypeExternal({
    required String projectDir,
    required String typeUsr,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<NameRegexRule> addNameRegex({
    required String projectDir,
    required String pattern,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<void> removeNameRegex({required String projectDir, required int id}) =>
      throw UnimplementedError('not used by this screen');

  @override
  Future<PathRegexRule> addPathRegex({
    required String projectDir,
    required String pattern,
  }) => throw UnimplementedError('not used by this screen');

  @override
  Future<void> removePathRegex({required String projectDir, required int id}) =>
      throw UnimplementedError('not used by this screen');
}
