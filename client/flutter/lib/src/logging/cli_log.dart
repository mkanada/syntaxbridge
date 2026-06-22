import 'dart:io';

void cliLog(String message) {
  stderr.writeln(
    '[syntax-bridge][ui][${DateTime.now().toIso8601String()}] $message',
  );
}
