import 'dart:io';
import 'dart:typed_data';

abstract class ScreenshotStorage {
  /// Persists a captured screen (PNG-encoded bytes) and returns where it
  /// was saved, so the caller can point the user at it.
  Future<String> save(Uint8List pngBytes);
}

class FileScreenshotStorage implements ScreenshotStorage {
  const FileScreenshotStorage();

  @override
  Future<String> save(Uint8List pngBytes) async {
    final home = Platform.environment['HOME'] ?? '.';
    final directory = Directory('$home/Pictures/syntax-bridge-screenshots');
    await directory.create(recursive: true);

    final file = File('${directory.path}/${_fileName()}');
    await file.writeAsBytes(pngBytes);
    return file.path;
  }

  String _fileName() {
    final timestamp = DateTime.now().toIso8601String().replaceAll(
      RegExp(r'[:.]'),
      '-',
    );
    return 'syntax-bridge-$timestamp.png';
  }
}
