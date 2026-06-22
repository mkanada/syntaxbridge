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

    expect(find.text('Syntax Bridge'), findsOneWidget);
    expect(find.text('Connected'), findsOneWidget);
    expect(find.text('syntax-bridge-server'), findsOneWidget);
  });

  testWidgets('creates a project and shows returned compilation units', (
    tester,
  ) async {
    final fakeClient = _FakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        buildLayers: [
          BuildLayer(
            index: 0,
            targets: [
              BuildTarget(
                id: 'syntax_bridge_core::@fixture',
                name: 'syntax_bridge_core',
                kind: 'STATIC_LIBRARY',
              ),
            ],
          ),
          BuildLayer(
            index: 1,
            targets: [
              BuildTarget(
                id: 'syntax_bridge_parser::@fixture',
                name: 'syntax_bridge_parser',
                kind: 'STATIC_LIBRARY',
              ),
            ],
          ),
          BuildLayer(
            index: 2,
            targets: [
              BuildTarget(
                id: 'syntax_bridge_app::@fixture',
                name: 'syntax_bridge_app',
                kind: 'EXECUTABLE',
              ),
            ],
          ),
        ],
        buildDependencyLayers: [
          BuildDependencyLayer(
            index: 0,
            items: [
              BuildDependencyItem(
                id: 'object:core',
                name: 'src/core.cpp',
                kind: 'OBJECT',
                dependencies: [],
              ),
              BuildDependencyItem(
                id: 'object:main',
                name: 'src/main.cpp',
                kind: 'OBJECT',
                dependencies: [],
              ),
            ],
          ),
          BuildDependencyLayer(
            index: 1,
            items: [
              BuildDependencyItem(
                id: 'target:app',
                name: 'syntax_bridge_app',
                kind: 'EXECUTABLE',
                dependencies: [
                  'src/main.cpp',
                  'syntax_bridge_core',
                  'syntax_bridge_parser',
                ],
              ),
            ],
          ),
        ],
        compilationUnits: [
          CompilationUnit(
            directory: '/tmp/projects/counter/build',
            file: '/tmp/projects/counter/input-source/fixture/main.cpp',
            command: 'clang++ -c main.cpp',
          ),
        ],
      ),
    );

    await tester.pumpWidget(SyntaxBridgeApp(serverClient: fakeClient));
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

    expect(fakeClient.createdProjectName, 'counter');
    expect(find.text('Execution log'), findsOneWidget);
    expect(find.text("Creating project 'counter'"), findsOneWidget);
    expect(find.text('Compilation units found: 1'), findsOneWidget);
    expect(find.text('Build plan'), findsOneWidget);
    expect(find.text('Layer 0'), findsOneWidget);
    expect(find.text('syntax_bridge_core'), findsWidgets);
    expect(find.text('Layer 1'), findsOneWidget);
    expect(find.text('syntax_bridge_parser'), findsWidgets);
    expect(find.text('Layer 2'), findsOneWidget);
    expect(find.text('syntax_bridge_app'), findsWidgets);
    expect(find.text('Build dependencies'), findsOneWidget);
    expect(find.text('Item'), findsWidgets);
    expect(find.text('Dependencies'), findsWidgets);
    expect(find.text('Dependency layer 0'), findsOneWidget);
    expect(find.text('src/core.cpp'), findsOneWidget);
    expect(find.text('No dependencies'), findsWidgets);
    expect(find.text('Dependency layer 1'), findsOneWidget);
    expect(
      find.text('src/main.cpp, syntax_bridge_core, syntax_bridge_parser'),
      findsOneWidget,
    );
    expect(find.text('Compilation units'), findsOneWidget);
    expect(find.text('input-source/fixture/main.cpp'), findsOneWidget);
    expect(
      find.text('/tmp/projects/counter/input-source/fixture/main.cpp'),
      findsNothing,
    );
    expect(find.text('clang++ -c main.cpp'), findsNothing);
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

    await tester.tap(find.byTooltip('Choose workspace directory'));
    await tester.pump();
    await tester.tap(find.byTooltip('Choose source archive'));
    await tester.pump();

    expect(pathPicker.workspacePickCount, 1);
    expect(pathPicker.archivePickCount, 1);
    expect(find.widgetWithText(TextField, '/tmp/projects'), findsOneWidget);
    expect(find.widgetWithText(TextField, '/tmp/source.zip'), findsOneWidget);
    expect(
      find.text('Workspace directory selected: /tmp/projects'),
      findsOneWidget,
    );
    expect(
      find.text('Source archive selected: /tmp/source.zip'),
      findsOneWidget,
    );
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

    expect(find.text('Project setup'), findsOneWidget);
    expect(find.text('Execution log'), findsOneWidget);
    expect(
      tester.getTopLeft(find.text('Project setup')).dx,
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
      greaterThan(tester.getTopLeft(find.text('Project setup')).dy),
    );

    await tester.tap(find.byTooltip('Dock Project setup panel'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dock right').last);
    await tester.pumpAndSettle();

    expect(
      tester.getTopLeft(find.text('Project setup')).dx,
      greaterThan(tester.getTopLeft(find.text('Syntax Bridge workspace')).dx),
    );
  });

  testWidgets('logs project creation failures with server details', (
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
      find.text(
        'Project creation failed: no CMakeLists.txt found under input-source',
      ),
      findsOneWidget,
    );
  });
}

class _FakeServerClient implements ServerClient {
  _FakeServerClient(this.status, {this.project, this.createError});

  final ServerStatus status;
  final CreatedProject? project;
  final Object? createError;
  String? createdProjectName;

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
          buildLayers: [],
          buildDependencyLayers: [],
          compilationUnits: [],
        );
  }
}

class _FakePathPicker implements PathPicker {
  _FakePathPicker({required this.workspaceDir, required this.sourceArchive});

  final String workspaceDir;
  final String sourceArchive;
  int workspacePickCount = 0;
  int archivePickCount = 0;

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
}
