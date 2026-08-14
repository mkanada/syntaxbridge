import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for US-5 (`docs/plans/User Steps.md`): the function/method
/// catalog and the caller-navigation panel for a selected function.
void main() {
  testWidgets('captures the function catalog table', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      functions: const [
        FunctionDeclaration(
          name: 'area',
          kind: FunctionDeclarationKind.method,
          namespace: 'geometry',
          signature: 'double geometry::Shape::area() const',
          file: '/tmp/projects/counter/input-source/fixture/shapes.h',
          line: 4,
          column: 19,
          isVirtual: true,
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

    await tester.tap(find.text('Functions'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us5-functions-view');
  });

  testWidgets('captures the callers panel for a selected function', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);
    const areaUsr = 'c:@N@geometry@S@Shape@F@area#1#';

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      project: const CreatedProject(
        name: 'counter',
        projectDir: '/tmp/projects/counter',
        inputSourceDir: '/tmp/projects/counter/input-source',
        compilationUnits: [],
        sourceFiles: [
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/shapes.h',
            kind: SourceFileKind.header,
          ),
          SourceFile(
            path: '/tmp/projects/counter/input-source/fixture/main.cpp',
            kind: SourceFileKind.translationUnit,
          ),
        ],
      ),
      functions: const [
        FunctionDeclaration(
          name: 'area',
          kind: FunctionDeclarationKind.method,
          namespace: 'geometry',
          signature: 'double geometry::Shape::area() const',
          file: '/tmp/projects/counter/input-source/fixture/shapes.h',
          line: 4,
          column: 19,
          usr: areaUsr,
          isVirtual: true,
        ),
      ],
      callerCounts: const {areaUsr: 1},
      callersByFunction: const {
        areaUsr: [
          CallEdge(
            callerUsr: 'c:@F@describe#',
            resolution: CallResolution.resolved(
              calleeUsr: areaUsr,
              isDynamicDispatch: true,
            ),
            file: '/tmp/projects/counter/input-source/fixture/main.cpp',
            line: 10,
            column: 19,
          ),
        ],
      },
      sourceFileContent: 'return shape.area();\n',
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.text('Functions'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('geometry::area'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us5-callers-view');
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
