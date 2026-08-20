import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for US-2 (`docs/plans/User Steps.md`): the source file list
/// and reading a file's content.
void main() {
  testWidgets('captures a source file open with its content shown', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
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
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/main.cpp',
            kind: SourceFileKind.translationUnit,
          ),
        ],
      ),
      sourceFileContent: 'struct Point {\n  int x;\n  int y;\n};',
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _createProject(tester);

    await tester.tap(find.text('types.h'));
    await tester.pumpAndSettle();

    final artifact = await captureTestScreen(
      tester,
      name: 'us2-source-file-viewer',
      boundaryKey: captureKey,
    );
    expect(artifact.existsSync(), isTrue);
    expect(artifact.lengthSync(), greaterThan(0));
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

void _configureDesktopSurface(WidgetTester tester) {
  tester.view
    ..devicePixelRatio = 1
    ..physicalSize = const Size(1280, 900);
  addTearDown(tester.view.reset);
}
