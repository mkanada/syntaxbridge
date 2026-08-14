import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for US-3 (`docs/plans/User Steps.md`): the project type
/// catalog and navigating from a type to its highlighted declaration.
void main() {
  testWidgets('captures the type catalog table', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      types: const [
        TypeDeclaration(
          name: 'Point',
          kind: TypeDeclarationKind.struct,
          namespace: 'geometry',
          file: '/tmp/projects/counter/input-source/fixture/types.h',
          line: 3,
          column: 8,
        ),
        TypeDeclaration(
          name: 'Shape',
          kind: TypeDeclarationKind.class_,
          namespace: 'geometry',
          file: '/tmp/projects/counter/input-source/fixture/shapes.h',
          line: 2,
          column: 7,
        ),
      ],
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.text('Types'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us3-types-view');
  });

  testWidgets("captures a type's source file open with its body highlighted", (
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
        ],
      ),
      types: const [
        TypeDeclaration(
          name: 'Point',
          kind: TypeDeclarationKind.struct,
          namespace: 'geometry',
          file: '/tmp/projects/counter/input-source/fixture/types.h',
          line: 3,
          column: 8,
          endLine: 6,
          endColumn: 1,
        ),
      ],
      sourceFileContent:
          'namespace geometry {\n\nstruct Point {\n  int x;\n  int y;\n};\n\n}',
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.text('Types'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('geometry::Point'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us3-type-source-highlighted');
  });
}

Future<void> _skipToIde(WidgetTester tester) async {
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
