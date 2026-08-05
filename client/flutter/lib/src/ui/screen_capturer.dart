import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

/// Captures a [RepaintBoundary]'s current content as PNG-encoded bytes.
abstract class ScreenCapturer {
  Future<Uint8List> capture(GlobalKey boundaryKey, {required double pixelRatio});
}

class RenderScreenCapturer implements ScreenCapturer {
  const RenderScreenCapturer();

  @override
  Future<Uint8List> capture(
    GlobalKey boundaryKey, {
    required double pixelRatio,
  }) async {
    final image = await _captureImage(boundaryKey, pixelRatio);
    final byteData = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();

    if (byteData == null) {
      throw StateError('could not encode screenshot as PNG');
    }

    return byteData.buffer.asUint8List();
  }

  /// Waits until the boundary has painted at least once, then captures it.
  ///
  /// The tap that triggers a capture starts the button's own ripple
  /// animation, which keeps the tree needing repaints for a few frames, and
  /// toImageSync can't be called until that settles (it throws while the
  /// boundary's layer isn't attached yet, or while mid-paint in debug and
  /// profile builds). Neither condition can be probed directly from outside
  /// the framework without touching debug-only or protected members, so
  /// this retries on whatever toImageSync throws instead.
  Future<ui.Image> _captureImage(
    GlobalKey boundaryKey,
    double pixelRatio,
  ) async {
    for (var attempt = 0; attempt < 120; attempt++) {
      final renderObject = boundaryKey.currentContext?.findRenderObject();
      if (renderObject is! RenderRepaintBoundary) {
        throw StateError('screen is not ready to capture');
      }

      try {
        return renderObject.toImageSync(pixelRatio: pixelRatio);
      } catch (_) {
        await _nextFrame();
      }
    }

    throw StateError('screen did not finish painting in time to capture');
  }

  Future<void> _nextFrame() {
    final completer = Completer<void>();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!completer.isCompleted) {
        completer.complete();
      }
    });
    WidgetsBinding.instance.scheduleFrame();
    return completer.future;
  }
}
