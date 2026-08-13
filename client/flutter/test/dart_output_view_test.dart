import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/dart_output_view.dart';

/// The panel that shows the Dart transpiled from whichever C++ source file
/// is currently open (US-8/PR6,
/// `docs/plans/primeiro-corte-e01-e03.md`).
void main() {
  testWidgets('prompts to transpile before any package has been requested', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(package: null, selectedCppPath: '/project/src/aritmetica.cpp'),
    );

    expect(find.textContaining('Transpile'), findsOneWidget);
  });

  testWidgets('prompts to open a file when no C++ file is selected', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(
        package: Future.value(
          const TranspiledPackage(
            packageName: 'e01_funcao_aritmetica',
            files: {'lib/aritmetica.dart': 'int soma(int a, int b) { ... }'},
          ),
        ),
        selectedCppPath: null,
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('Open a C++ source file'), findsOneWidget);
  });

  testWidgets('shows the matching generated Dart file once transpiled', (
    tester,
  ) async {
    const dartSource = 'int soma(int a, int b) {\n  return a + b;\n}\n';
    await tester.pumpWidget(
      _host(
        package: Future.value(
          const TranspiledPackage(
            packageName: 'e01_funcao_aritmetica',
            files: {
              'pubspec.yaml': 'name: e01_funcao_aritmetica\n',
              'lib/aritmetica.dart': dartSource,
            },
          ),
        ),
        selectedCppPath: '/project/input-source/src/aritmetica.cpp',
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('int soma(int a, int b)'), findsOneWidget);
    expect(find.textContaining('return a + b;'), findsOneWidget);
  });

  testWidgets('reports when the open C++ file has no matching Dart output', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(
        package: Future.value(
          const TranspiledPackage(
            packageName: 'e02_controle_de_fluxo',
            files: {'lib/controle.dart': 'int divide_inteiro(int a, int b);'},
          ),
        ),
        selectedCppPath: '/project/input-source/src/outro.cpp',
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('No generated Dart'), findsOneWidget);
  });

  testWidgets('reports a failed transpile without crashing', (tester) async {
    // `..ignore()` tells Dart this rejection is intentionally handled
    // asynchronously (by `FutureBuilder`, once it attaches its listener
    // during build) rather than left unhandled — without it, the test
    // framework's zone error guard flags the rejection before that
    // listener ever gets a chance to run.
    final failingPackage = Future<TranspiledPackage>.error('boom')..ignore();
    await tester.pumpWidget(
      _host(
        package: failingPackage,
        selectedCppPath: '/project/src/aritmetica.cpp',
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('Failed to transpile'), findsOneWidget);
  });

  test('matchingDartPath lowercases the C++ file stem', () {
    expect(
      matchingDartPath('/project/input-source/src/Aritmetica.cpp'),
      'lib/aritmetica.dart',
    );
    expect(matchingDartPath('controle.cpp'), 'lib/controle.dart');
  });

  // Regression test: `matchingDartPath` used to only lowercase the stem,
  // while the server's `sanitize_identifier`
  // (`crates/server/src/emit/dart.rs`) also folds diacritics and replaces
  // every character outside `[a-z0-9_]` with `_` — a stem with a hyphen,
  // space, or accented letter produced a different key on each side, so
  // the panel reported "no generated Dart" even when transpilation
  // succeeded. `matchingDartPath` must mirror the server's rule exactly.
  test('matchingDartPath mirrors the server\'s sanitize_identifier for '
      'punctuation and diacritics', () {
    expect(matchingDartPath('my-file.cpp'), 'lib/my_file.dart');
    expect(matchingDartPath('my file.cpp'), 'lib/my_file.dart');
    expect(matchingDartPath('Função.cpp'), 'lib/funcao.dart');
  });
}

Widget _host({
  required Future<TranspiledPackage>? package,
  required String? selectedCppPath,
}) {
  return MaterialApp(
    home: Scaffold(
      body: DartOutputView(package: package, selectedCppPath: selectedCppPath),
    ),
  );
}
