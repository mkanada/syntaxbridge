import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for US-4 (`docs/plans/User Steps.md`): usage counts on the
/// type list and the usage-navigation panel for a selected type.
void main() {
  testWidgets('captures the usages panel for a selected type', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);
    const pointUsr = 'c:@N@geometry@S@Point';

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
      types: const [
        TypeDeclaration(
          name: 'Point',
          kind: TypeDeclarationKind.struct,
          namespace: 'geometry',
          file: '/tmp/projects/counter/input-source/fixture/types.h',
          line: 3,
          column: 8,
          usr: pointUsr,
        ),
      ],
      usageCounts: const {pointUsr: 1},
      usagesByType: const {
        pointUsr: [
          TypeUsage(
            typeUsr: pointUsr,
            kind: TypeUsageKind.variableDeclaration,
            file: '/tmp/projects/counter/input-source/fixture/main.cpp',
            line: 4,
            column: 1,
          ),
        ],
      },
      sourceFileContent: 'Point origin;\n',
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

    final artifact = await captureTestScreen(
      tester,
      name: 'us4-usages-view',
      boundaryKey: captureKey,
    );
    expect(artifact.existsSync(), isTrue);
    expect(artifact.lengthSync(), greaterThan(0));
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

void _configureDesktopSurface(WidgetTester tester) {
  tester.view
    ..devicePixelRatio = 1
    ..physicalSize = const Size(1280, 900);
  addTearDown(tester.view.reset);
}
