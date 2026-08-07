import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

void main() {
  testWidgets('shows connected status when the Rust server is healthy', (
    tester,
  ) async {
    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );

    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Syntax Bridge'), findsOneWidget);
    expect(find.text('Connected'), findsOneWidget);
    expect(find.text('syntax-bridge-server'), findsOneWidget);
  });

  testWidgets('renders the project workflow inside IDE chrome', (tester) async {
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Syntax Bridge workspace'), findsOneWidget);
    expect(find.text('Explorer'), findsOneWidget);
    expect(find.text('Execution log'), findsOneWidget);
    expect(find.byTooltip('Refresh'), findsWidgets);
    expect(find.byTooltip('Capture screen'), findsOneWidget);
  });

  testWidgets('omits IDE chrome that syntax-bridge does not implement', (
    tester,
  ) async {
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    // Decorative VS Code menu bar: none of these open anything.
    for (final menu in ['File', 'Edit', 'Search', 'Run', 'Window']) {
      expect(find.text(menu), findsNothing, reason: 'dead "$menu" menu');
    }

    // Activity rail entries for features the app does not have.
    for (final rail in [
      'Search',
      'Source control',
      'Debug',
      'Extensions',
      'Account',
      'Settings',
    ]) {
      expect(find.byTooltip(rail), findsNothing, reason: 'dead "$rail" rail');
    }

    // Toolbar buttons wired to nothing.
    expect(find.byTooltip('Run pipeline'), findsNothing);
    expect(find.byTooltip('Settings'), findsNothing);

    // Search box that searches nothing.
    expect(find.text('Search workspace'), findsNothing);

    // Fabricated status bar readings.
    expect(find.text('main'), findsNothing);
    expect(find.text('0 errors, 0 warnings'), findsNothing);
    expect(find.text('UTF-8'), findsNothing);
    expect(find.text('Flatpak pending'), findsNothing);

    // Hardcoded pipeline snippet presented as project state.
    expect(find.text('workspace.syntax-bridge'), findsNothing);
    expect(find.text('  build_graph: ready'), findsNothing);
    expect(find.text('  compilation_units: pending'), findsNothing);
  });

  testWidgets('creates a project and shows returned source files', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
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
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/main.cpp',
            kind: SourceFileKind.translationUnit,
          ),
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/types.h',
            kind: SourceFileKind.header,
          ),
        ],
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

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

    expect(fakeClient.createdProjectName, 'counter');
    expect(find.text('Execution log'), findsOneWidget);
    expect(find.text('Source files'), findsOneWidget);
    expect(find.text('input-source/fixture/main.cpp'), findsOneWidget);
    expect(find.text('input-source/fixture/types.h'), findsOneWidget);
    expect(
      find.text('/tmp/projects/counter/input-source/fixture/main.cpp'),
      findsNothing,
    );
    expect(find.text('clang++ -c main.cpp'), findsNothing);
  });

  testWidgets('opens a source file and displays its content on click', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/types.h',
            kind: SourceFileKind.header,
          ),
        ],
      ),
      sourceFileContent: 'struct Point {\n  int x;\n  int y;\n};',
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

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

    await tester.tap(find.text('input-source/fixture/types.h'));
    await tester.pumpAndSettle();

    expect(
      fakeClient.readSourceFilePath,
      '/tmp/projects/counter/input-source/fixture/types.h',
    );
    expect(find.text('struct Point {'), findsOneWidget);
    expect(find.text('  int x;'), findsOneWidget);
    expect(find.text('types.h'), findsWidgets);
  });

  testWidgets('opens path pickers for workspace and source archive', (
    tester,
  ) async {
    final pathPicker = _FakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
        pathPicker: pathPicker,
      ),
    );
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

    await tester.tap(find.byTooltip('Choose workspace directory'));
    await tester.pump();
    await tester.tap(find.byTooltip('Choose source archive'));
    await tester.pump();

    expect(pathPicker.workspacePickCount, 1);
    expect(pathPicker.archivePickCount, 1);
    expect(find.widgetWithText(TextField, '/tmp/projects'), findsOneWidget);
    expect(find.widgetWithText(TextField, '/tmp/source.zip'), findsOneWidget);
  });

  testWidgets('closes, reopens, and docks IDE panels with one click', (
    tester,
  ) async {
    tester.view
      ..devicePixelRatio = 1
      ..physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    expect(find.text('Explorer'), findsOneWidget);
    expect(find.text('Execution log'), findsOneWidget);
    expect(
      tester.getTopLeft(find.text('Explorer')).dx,
      lessThan(tester.getTopLeft(find.text('Execution log')).dx),
    );

    await tester.tap(find.byTooltip('Close Execution log panel'));
    await tester.pumpAndSettle();

    expect(find.byTooltip('Close Execution log panel'), findsNothing);
    expect(
      find.widgetWithText(OutlinedButton, 'Execution log'),
      findsOneWidget,
    );

    await tester.tap(find.widgetWithText(OutlinedButton, 'Execution log'));
    await tester.pumpAndSettle();

    expect(find.byTooltip('Close Execution log panel'), findsOneWidget);

    await tester.tap(find.byTooltip('Dock Execution log panel'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dock bottom').last);
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.text('Execution log')).dy,
      greaterThan(tester.getTopLeft(find.text('Explorer')).dy),
    );

    await tester.tap(find.byTooltip('Dock Explorer panel'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dock right').last);
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.text('Explorer')).dx,
      greaterThan(tester.getTopLeft(find.text('Syntax Bridge workspace')).dx),
    );
  });

  testWidgets('shows project creation failures with server details', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      createError: const ProjectCreationException(
        'no CMakeLists.txt found under input-source',
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();
    await _openNewProjectPage(tester);

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

    expect(find.text('Project creation failed'), findsOneWidget);
    expect(
      find.text('no CMakeLists.txt found under input-source'),
      findsOneWidget,
    );
  });

  testWidgets('shows an empty state when there are no recent projects', (
    tester,
  ) async {
    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('No recent projects yet'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'New project'), findsOneWidget);
    expect(
      find.widgetWithText(OutlinedButton, 'Import project'),
      findsOneWidget,
    );
  });

  testWidgets('lists recent projects and opens one without re-ingesting', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      recentProjects: const [
        RecentProject(
          name: 'counter',
          projectDir: '/tmp/projects/counter',
          sourceLanguage: 'cpp',
          targetLanguage: 'dart',
          lastIngestStatus: 'success',
        ),
      ],
      openProjectResult: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/main.cpp',
            kind: SourceFileKind.translationUnit,
          ),
        ],
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
    await tester.pumpAndSettle();

    expect(find.text('counter'), findsOneWidget);
    expect(find.text('/tmp/projects/counter'), findsOneWidget);

    await tester.tap(find.text('counter'));
    await tester.pumpAndSettle();

    expect(fakeClient.openedProjectDir, '/tmp/projects/counter');
    expect(find.text('Explorer'), findsOneWidget);
    expect(find.text('input-source/fixture/main.cpp'), findsOneWidget);
  });

  testWidgets('imports an existing project through the modal dialog', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      openProjectResult: const CreatedProject(
        name: 'verovio',
        projectDir: '/home/user/syntax-bridge-projects/verovio',
        inputSourceDir: '/home/user/syntax-bridge-projects/verovio/input-source',
        compilationUnits: [],
        sourceFiles: [],
      ),
    );
    final pathPicker = _FakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
      existingProjectDir: '/home/user/syntax-bridge-projects/verovio',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(serverClient: fakeClient, pathPicker: pathPicker),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(OutlinedButton, 'Import project'));
    await tester.pumpAndSettle();

    expect(find.text(_importDialogPrompt), findsOneWidget);

    // Tapping outside the dialog must not dismiss it: the import modal is
    // intentionally non-dismissible until it succeeds or is cancelled.
    await tester.tapAt(const Offset(5, 5));
    await tester.pumpAndSettle();
    expect(find.text(_importDialogPrompt), findsOneWidget);

    await tester.tap(find.byTooltip('Choose project directory'));
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Import'));
    await tester.pumpAndSettle();

    expect(
      fakeClient.openedProjectDir,
      '/home/user/syntax-bridge-projects/verovio',
    );
    expect(find.text(_importDialogPrompt), findsNothing);
    expect(find.text('Explorer'), findsOneWidget);
  });

  testWidgets('keeps the import dialog open and shows the error on failure', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      openProjectError: const ProjectCreationException(
        'no syntax-bridge project found at /tmp/not-a-project',
      ),
    );
    final pathPicker = _FakePathPicker(
      workspaceDir: '/tmp/projects',
      sourceArchive: '/tmp/source.zip',
      existingProjectDir: '/tmp/not-a-project',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(serverClient: fakeClient, pathPicker: pathPicker),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(OutlinedButton, 'Import project'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Choose project directory'));
    await tester.pump();
    await tester.tap(find.widgetWithText(FilledButton, 'Import'));
    await tester.pumpAndSettle();

    expect(find.text(_importDialogPrompt), findsOneWidget);
    expect(
      find.text(
        'Import failed: no syntax-bridge project found at /tmp/not-a-project',
      ),
      findsOneWidget,
    );
  });

  testWidgets('captures the screen and logs where it was saved', (
    tester,
  ) async {
    final screenshotStorage = _FakeScreenshotStorage(
      savedPath: '/tmp/screenshots/syntax-bridge-fake.png',
    );

    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
        screenshotStorage: screenshotStorage,
        screenCapturer: _FakeScreenCapturer(),
      ),
    );
    await tester.pumpAndSettle();
    await _skipToIde(tester);

    await tester.tap(find.byTooltip('Capture screen'));
    await tester.pumpAndSettle();

    expect(screenshotStorage.savedBytes, isNotNull);
    expect(screenshotStorage.savedBytes, isNotEmpty);
    expect(
      find.text('Screenshot saved: /tmp/screenshots/syntax-bridge-fake.png'),
      findsOneWidget,
    );
  });
}

const _importDialogPrompt =
    'Point to a directory that already contains a syntax-bridge project.';

Future<void> _openNewProjectPage(WidgetTester tester) async {
  await tester.tap(find.widgetWithText(FilledButton, 'New project'));
  await tester.pumpAndSettle();
}

/// Navigates from the landing page through the new-project form using
/// placeholder values, landing in the IDE. Use this in tests that only care
/// about reaching the IDE, not about the creation form itself.
Future<void> _skipToIde(WidgetTester tester) async {
  await _openNewProjectPage(tester);
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

class _FakeScreenCapturer implements ScreenCapturer {
  @override
  Future<Uint8List> capture(
    GlobalKey boundaryKey, {
    required double pixelRatio,
  }) async {
    return Uint8List.fromList([1, 2, 3, 4]);
  }
}

class _FakeScreenshotStorage implements ScreenshotStorage {
  _FakeScreenshotStorage({required this.savedPath});

  final String savedPath;
  Uint8List? savedBytes;

  @override
  Future<String> save(Uint8List pngBytes) async {
    savedBytes = pngBytes;
    return savedPath;
  }
}

class _FakeServerClient implements ServerClient {
  _FakeServerClient(
    this.status, {
    this.project,
    this.createError,
    this.sourceFileContent = '',
    this.recentProjects = const <RecentProject>[],
    this.openProjectResult,
    this.openProjectError,
  });

  final ServerStatus status;
  final CreatedProject? project;
  final Object? createError;
  final String sourceFileContent;
  final List<RecentProject> recentProjects;
  final CreatedProject? openProjectResult;
  final Object? openProjectError;
  String? createdProjectName;
  String? readSourceFilePath;
  String? openedProjectDir;

  @override
  Future<ServerStatus> health() async => status;

  @override
  Future<CreatedProject> createProject(CreateProjectInput input) async {
    createdProjectName = input.name;
    final error = createError;
    if (error != null) {
      throw error;
    }

    return project ??
        const CreatedProject(
          name: 'empty',
          projectDir: '',
          inputSourceDir: '',
          compilationUnits: [],
        );
  }

  @override
  Future<List<RecentProject>> listRecentProjects() async => recentProjects;

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
}

class _FakePathPicker implements PathPicker {
  _FakePathPicker({
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
