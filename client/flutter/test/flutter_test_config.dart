import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

/// `flutter test` renders text — and icons, which are just glyphs in an icon
/// font — with a placeholder "tofu" font (a solid box per glyph) unless a
/// real font is registered, otherwise every screenshot this suite captures
/// would show boxes instead of legible text and icons. Loads vendored,
/// redistributable fonts (`test/fonts/`) before any test runs, so it works
/// the same in CI as on a dev machine, without depending on fonts installed
/// on the host or network access:
///  - DejaVu Sans/Sans Mono (Bitstream Vera license, `test/fonts/LICENSE`)
///    stand in for the app's default and monospace text.
///  - MaterialIcons-Regular.otf (Apache 2.0, vendored straight from this
///    Flutter SDK's own cache — the exact font `uses-material-design: true`
///    already bundles into every real build) renders `Icon(Icons.*)`.
Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  TestWidgetsFlutterBinding.ensureInitialized();
  await _loadFont('Roboto', 'test/fonts/DejaVuSans.ttf');
  await _loadFont('monospace', 'test/fonts/DejaVuSansMono.ttf');
  await _loadFont('MaterialIcons', 'test/fonts/MaterialIcons-Regular.otf');
  await testMain();
}

Future<void> _loadFont(String family, String path) async {
  final bytes = await File(path).readAsBytes();
  final fontLoader = FontLoader(family)
    ..addFont(Future.value(ByteData.sublistView(Uint8List.fromList(bytes))));
  await fontLoader.load();
}
