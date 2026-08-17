import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for the Validation panel (US-9): `dart analyze` findings
/// against the transpiled project, each showing where it resolves back to
/// in the original C++ when a declaration could be located, and the
/// success state when there are none.
void main() {
  testWidgets('captures the validation panel with diagnostics', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    const cppPath = '/tmp/projects/counter/input-source/fixture/aritmetica.cpp';
    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      diagnostics: const [
        DartDiagnostic(
          severity: DartDiagnosticSeverity.error,
          message: "Undefined name 'naoexiste'.",
          dartFile: 'lib/aritmetica.dart',
          dartLine: 2,
          origin: CppOrigin(file: cppPath, line: 2, column: 14),
        ),
        DartDiagnostic(
          severity: DartDiagnosticSeverity.warning,
          message: 'Unused import.',
          dartFile: 'lib/aritmetica.dart',
          dartLine: 1,
          origin: null,
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

    await tester.tap(find.byTooltip('Validate'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us9-validation-diagnostics');
  });

  testWidgets('captures the success state when there are no diagnostics', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.byTooltip('Validate'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'us9-validation-clean');
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
