import 'dart:io';
import 'dart:typed_data';
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
  final width = image.width;
  final height = image.height;

  final artifact = await tester.runAsync(() async {
    final pixels = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
    image.dispose();

    if (pixels == null) {
      throw StateError('Could not read screenshot "$name" pixels.');
    }

    final directory = Directory(outputDirectory);
    await directory.create(recursive: true);

    final artifact = File('${directory.path}/$safeName.bmp');
    final bmp = _bmpFromRgba(pixels, width, height);
    await artifact.writeAsBytes(bmp);
    return artifact;
  });

  if (artifact == null) {
    throw StateError('Could not write screenshot "$name".');
  }

  return artifact;
}

Uint8List _bmpFromRgba(ByteData pixels, int width, int height) {
  const fileHeaderSize = 14;
  const dibHeaderSize = 40;
  const bytesPerPixel = 4;
  const pixelOffset = fileHeaderSize + dibHeaderSize;
  final pixelBytesLength = width * height * bytesPerPixel;
  final output = Uint8List(pixelOffset + pixelBytesLength);
  final data = ByteData.sublistView(output);

  output[0] = 0x42;
  output[1] = 0x4d;
  data.setUint32(2, output.length, Endian.little);
  data.setUint32(10, pixelOffset, Endian.little);
  data.setUint32(14, dibHeaderSize, Endian.little);
  data.setInt32(18, width, Endian.little);
  data.setInt32(22, height, Endian.little);
  data.setUint16(26, 1, Endian.little);
  data.setUint16(28, 32, Endian.little);
  data.setUint32(34, pixelBytesLength, Endian.little);

  var outputOffset = pixelOffset;
  for (var y = height - 1; y >= 0; y -= 1) {
    for (var x = 0; x < width; x += 1) {
      final inputOffset = (y * width + x) * bytesPerPixel;
      output[outputOffset] = pixels.getUint8(inputOffset + 2);
      output[outputOffset + 1] = pixels.getUint8(inputOffset + 1);
      output[outputOffset + 2] = pixels.getUint8(inputOffset);
      output[outputOffset + 3] = pixels.getUint8(inputOffset + 3);
      outputOffset += bytesPerPixel;
    }
  }

  return output;
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
