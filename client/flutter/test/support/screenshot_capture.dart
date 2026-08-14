import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

const _defaultScreenshotDirectory = 'build/test-screenshots';

Future<File> captureTestScreen(
  WidgetTester tester, {
  required String name,
  required GlobalKey boundaryKey,
  String outputDirectory = _defaultScreenshotDirectory,
  double pixelRatio = 1,
}) async {
  final safeName = _safeScreenshotName(name);
  final context = boundaryKey.currentContext;

  if (context == null) {
    throw StateError('Screenshot boundary "$name" is not mounted.');
  }

  final renderObject = context.findRenderObject();
  if (renderObject is! RenderRepaintBoundary) {
    throw StateError('Screenshot boundary "$name" is not a RepaintBoundary.');
  }

  await tester.pump();

  assert(!renderObject.debugNeedsPaint);
  final image = renderObject.toImageSync(pixelRatio: pixelRatio);

  final artifact = await tester.runAsync(() async {
    final pixels = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();

    if (pixels == null) {
      throw StateError('Could not encode screenshot "$name" as PNG.');
    }

    final directory = Directory(outputDirectory);
    await directory.create(recursive: true);

    final artifact = File('${directory.path}/$safeName.png');
    await artifact.writeAsBytes(pixels.buffer.asUint8List());
    return artifact;
  });

  if (artifact == null) {
    throw StateError('Could not write screenshot "$name".');
  }

  return artifact;
}

String _safeScreenshotName(String name) {
  final normalized = name.trim().toLowerCase().replaceAll(
    RegExp(r'[^a-z0-9._-]+'),
    '-',
  );
  final safe = normalized.replaceAll(RegExp(r'^[-.]+|[-.]+$'), '');

  if (safe.isEmpty) {
    throw ArgumentError.value(name, 'name', 'must contain a safe file name');
  }

  return safe;
}
