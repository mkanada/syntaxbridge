import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for the pointer catalog (Parte 1 of
/// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`): every raw pointer
/// declared in the project, and the empty state when a project has none.
void main() {
  testWidgets('captures the pointer catalog table', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      pointers: const [
        PointerDeclaration(
          kind: PointerDeclarationKind.parameter,
          shape: PointerShape.scalar,
          name: 'forma',
          pointeeTypeName: 'Forma',
          pointeeUsr: 'c:@S@Forma',
          file: '/tmp/projects/counter/input-source/fixture/desenha.cpp',
          line: 5,
          column: 20,
        ),
        PointerDeclaration(
          kind: PointerDeclarationKind.field,
          shape: PointerShape.scalar,
          name: 'atual',
          pointeeTypeName: 'Forma',
          pointeeUsr: 'c:@S@Forma',
          file: '/tmp/projects/counter/input-source/fixture/painel.h',
          line: 3,
          column: 12,
        ),
        PointerDeclaration(
          kind: PointerDeclarationKind.local,
          shape: PointerShape.doublePointer,
          name: 'duplo',
          pointeeTypeName: 'Forma *',
          pointeeUsr: 'c:@S@Forma',
          file: '/tmp/projects/counter/input-source/fixture/desenha.cpp',
          line: 6,
          column: 12,
        ),
        PointerDeclaration(
          kind: PointerDeclarationKind.local,
          shape: PointerShape.functionPointer,
          name: 'callback',
          pointeeTypeName: 'void (int)',
          file: '/tmp/projects/counter/input-source/fixture/desenha.cpp',
          line: 7,
          column: 10,
        ),
        PointerDeclaration(
          kind: PointerDeclarationKind.returnType,
          shape: PointerShape.scalar,
          name: 'fabrica',
          pointeeTypeName: 'Forma',
          pointeeUsr: 'c:@S@Forma',
          file: '/tmp/projects/counter/input-source/fixture/desenha.cpp',
          line: 9,
          column: 1,
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

    await tester.tap(find.text('Pointers'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'pointer-catalog-view');
  });

  testWidgets('captures the empty state when a project has no pointers', (
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

    await tester.tap(find.text('Pointers'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'pointer-catalog-empty');
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
