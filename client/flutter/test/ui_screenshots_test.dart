import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import 'support/screenshot_capture.dart';

void main() {
  testWidgets('captures the connected project screen', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: _FakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final artifact = await captureTestScreen(
      tester,
      name: 'syntax-bridge-connected',
      boundaryKey: captureKey,
    );

    _expectScreenshotArtifact(artifact);
  });

  testWidgets('captures the project creation result screen', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(
          serverClient: _FakeServerClient(
            const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
            project: _projectWithCompilationUnits,
          ),
        ),
      ),
    );
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

    final artifact = await captureTestScreen(
      tester,
      name: 'syntax-bridge-project-created',
      boundaryKey: captureKey,
    );

    _expectScreenshotArtifact(artifact);
  });
}

void _configureDesktopSurface(WidgetTester tester) {
  tester.view
    ..devicePixelRatio = 1
    ..physicalSize = const Size(1280, 900);
  addTearDown(tester.view.reset);
}

void _expectScreenshotArtifact(File artifact) {
  expect(artifact.existsSync(), isTrue);
  expect(artifact.lengthSync(), greaterThan(54));
}

class _FakeServerClient implements ServerClient {
  _FakeServerClient(this.status, {this.project});

  final ServerStatus status;
  final CreatedProject? project;

  @override
  Future<ServerStatus> health() async => status;

  @override
  Future<CreatedProject> createProject(CreateProjectInput input) async {
    return project ??
        CreatedProject(
          name: input.name,
          projectDir: '${input.workspaceDir}/${input.name}',
          inputSourceDir: '${input.workspaceDir}/${input.name}/input-source',
          buildLayers: const [],
          buildDependencyLayers: const [],
          compilationUnits: const [],
        );
  }
}

const _projectWithCompilationUnits = CreatedProject(
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
          dependencies: ['src/main.cpp', 'syntax_bridge_core'],
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
);
