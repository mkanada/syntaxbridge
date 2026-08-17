import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for the "Extern" screen
/// (`docs/plans/lista-de-externos.md`) — not a `docs/plans/User Steps.md`
/// US-N step, a recurso negociado fora do roteiro, but the same screenshot
/// discipline `AGENTS.md` requires for every new client screen still
/// applies: the full screen with items from every source, the regexp
/// editor with a pattern filled in, and a type marked external from the
/// Types view (an intermediate state of the marking flow, not just the
/// Extern screen's own final state).
void main() {
  testWidgets('captures the extern view with items from every source', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      externalListing: const ExternalListing(
        statuses: [
          ExternalStatus(
            usr: 'c:@S@Humlib',
            effective: true,
            sources: [ExternalSource(kind: ExternalSourceKind.manualInclude)],
          ),
          ExternalStatus(
            usr: 'c:@N@humlib@S@HumdrumFile',
            effective: true,
            sources: [
              ExternalSource(
                kind: ExternalSourceKind.nameRegex,
                pattern: '^humlib::',
              ),
            ],
          ),
          ExternalStatus(
            usr: 'c:@F@miniz_deflate#',
            effective: true,
            sources: [
              ExternalSource(
                kind: ExternalSourceKind.pathRegex,
                pattern: '^third_party/',
              ),
            ],
          ),
          ExternalStatus(
            usr: 'c:@F@VrvFontStyle#',
            effective: true,
            sources: [
              ExternalSource(kind: ExternalSourceKind.autoUndefinedFunction),
            ],
          ),
          ExternalStatus(
            usr: 'c:@F@ignorado#',
            effective: false,
            sources: [
              ExternalSource(
                kind: ExternalSourceKind.pathRegex,
                pattern: '^third_party/',
              ),
              ExternalSource(kind: ExternalSourceKind.manualExclude),
            ],
          ),
        ],
        nameRegexes: [
          NameRegexRule(id: 1, pattern: '^humlib::', createdAt: '0'),
        ],
        pathRegexes: [
          PathRegexRule(id: 1, pattern: '^third_party/', createdAt: '0'),
        ],
      ),
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.text('Extern'));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'externos-view');
  });

  testWidgets('captures a name regexp typed into the extern editor', (
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

    await tester.tap(find.text('Extern'));
    await tester.pumpAndSettle();

    // The name-regexp field is the first of the two `TextField`s the
    // Extern screen renders (name-regexp section above path-regexp).
    await tester.enterText(find.byType(TextField).first, '^humlib::');
    await tester.pump();

    await _capture(tester, captureKey, 'externos-name-regex-filled');
  });

  testWidgets('captures a type marked external from the types view', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      types: const [
        TypeDeclaration(
          name: 'Shape',
          kind: TypeDeclarationKind.class_,
          file: '/tmp/projects/counter/input-source/fixture/shapes.h',
          line: 3,
          column: 7,
          usr: 'c:@S@Shape',
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

    await tester.tap(find.byIcon(Icons.link));
    await tester.pumpAndSettle();

    await _capture(tester, captureKey, 'types-view-item-marked-external');
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
