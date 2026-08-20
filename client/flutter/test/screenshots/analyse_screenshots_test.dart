import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

import '../support/screenshot_capture.dart';
import 'support/fake_server_client.dart';

/// Screenshots for "Analyse" (item 2, `docs/prompts/2026-08-19-mudanca-interacao.md`):
/// the toolbar icon's three states — awaiting analysis right after a fresh
/// ingest, in progress once the user starts it, and done once it succeeds.
void main() {
  testWidgets('captures the toolbar right after ingestion, awaiting analysis', (
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

    expect(find.byTooltip('Analyse'), findsOneWidget);
    await _capture(tester, captureKey, 'analyse-awaiting');
  });

  testWidgets('captures the toolbar while analysis is running', (tester) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      analysisJobStatuses: const [
        AnalysisJobStatus(state: AnalysisJobState.running),
      ],
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.byTooltip('Analyse'));
    await tester.pump();

    expect(find.byTooltip('Analysing...'), findsOneWidget);
    await _capture(tester, captureKey, 'analyse-running');
  });

  testWidgets('captures the toolbar once analysis has completed', (
    tester,
  ) async {
    final captureKey = GlobalKey();
    _configureDesktopSurface(tester);

    final fakeClient = ScreenshotFakeServerClient(
      const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
      analysisJobStatuses: const [
        AnalysisJobStatus(state: AnalysisJobState.succeeded),
      ],
    );

    await tester.pumpWidget(
      RepaintBoundary(
        key: captureKey,
        child: SyntaxBridgeApp(serverClient: fakeClient),
      ),
    );
    await _skipToIde(tester);

    await tester.tap(find.byTooltip('Analyse'));
    await tester.pumpAndSettle(const Duration(milliseconds: 500));

    expect(find.byTooltip('Re-analyse'), findsOneWidget);
    await _capture(tester, captureKey, 'analyse-done');
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
