import 'package:flutter/material.dart';
import 'package:syntax_bridge/src/rust/api/diagnostics.dart';
import 'package:syntax_bridge/src/rust/api/simple.dart';
import 'package:syntax_bridge/src/rust/frb_generated.dart';

Future<void> main() async {
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key, this.rustMessageOverride, this.diagnosticLinesOverride});

  final String? rustMessageOverride;
  final List<String>? diagnosticLinesOverride;

  @override
  Widget build(BuildContext context) {
    final rustMessage = rustMessageOverride ?? greet(name: 'Syntax Bridge');
    final diagnosticLines =
        diagnosticLinesOverride ??
        _runStartupDiagnosticsWithCommandLineLog();

    return MaterialApp(
      title: 'Syntax Bridge',
      theme: ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: Colors.teal)),
      home: Scaffold(
        appBar: AppBar(title: const Text('Syntax Bridge')),
        body: Center(
          child: Card(
            margin: const EdgeInsets.all(24),
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const Text('Flutter chamando Rust via flutter_rust_bridge'),
                  const SizedBox(height: 12),
                  Text(
                    rustMessage,
                    style: const TextStyle(
                      fontSize: 24,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                  const SizedBox(height: 24),
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Text(
                          'Startup diagnostics',
                          style: TextStyle(fontWeight: FontWeight.bold),
                        ),
                        const SizedBox(height: 8),
                        for (final line in diagnosticLines)
                          Text(
                            line,
                            style: const TextStyle(fontFamily: 'monospace'),
                          ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

String _formatDiagnosticCheck(DiagnosticCheck check) {
  return switch (check.status) {
    DiagnosticStatus.ok => 'Checking ${check.tool}...ok',
    DiagnosticStatus.failed when check.message != null =>
      'Checking ${check.tool}...failed: ${check.message}',
    DiagnosticStatus.failed => 'Checking ${check.tool}...failed',
  };
}

List<String> _runStartupDiagnosticsWithCommandLineLog() {
  final lines = runStartupDiagnostics().map(_formatDiagnosticCheck).toList();

  for (final line in lines) {
    debugPrint(line);
  }

  return lines;
}
