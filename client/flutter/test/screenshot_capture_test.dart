import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/screenshot_capture.dart';

void main() {
  testWidgets('captures a rendered screen as an image artifact', (
    tester,
  ) async {
    final captureKey = GlobalKey();

    await tester.pumpWidget(
      MaterialApp(
        home: RepaintBoundary(
          key: captureKey,
          child: const Scaffold(
            body: Center(child: Text('Syntax Bridge screenshot probe')),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final artifact = await captureTestScreen(
      tester,
      name: 'screenshot-probe',
      boundaryKey: captureKey,
      // Infra self-test, not a product screen: keep it out of the gallery
      // directory the real UI screenshots are gathered from.
      outputDirectory: 'build/test-screenshots-infra',
    );

    expect(artifact.path, endsWith('screenshot-probe.png'));
    expect(artifact.existsSync(), isTrue);
    expect(artifact.lengthSync(), greaterThan(0));
  });
}
