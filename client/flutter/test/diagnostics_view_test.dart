import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/src/project/project_models.dart';
import 'package:syntax_bridge/src/ui/diagnostics_view.dart';

void main() {
  const errorWithOrigin = DartDiagnostic(
    severity: DartDiagnosticSeverity.error,
    message: "Undefined name 'naoexiste'.",
    dartFile: 'lib/aritmetica.dart',
    dartLine: 2,
    origin: CppOrigin(
      file: '/workspace/src/aritmetica.cpp',
      line: 1,
      column: 1,
    ),
  );
  const warningWithoutOrigin = DartDiagnostic(
    severity: DartDiagnosticSeverity.warning,
    message: 'Unused import.',
    dartFile: 'lib/aritmetica.dart',
    dartLine: 1,
    origin: null,
  );

  testWidgets('shows a placeholder before validation has run', (tester) async {
    await tester.pumpWidget(_host(null));

    expect(
      find.text('Click "Validate" to run dart analyze on this project.'),
      findsOneWidget,
    );
  });

  testWidgets('shows a success message when there are no diagnostics', (
    tester,
  ) async {
    await tester.pumpWidget(_host(Future.value(const [])));
    await tester.pumpAndSettle();

    expect(find.text('No issues found'), findsOneWidget);
  });

  testWidgets('lists each diagnostic with its severity and location', (
    tester,
  ) async {
    await tester.pumpWidget(
      _host(Future.value(const [errorWithOrigin, warningWithoutOrigin])),
    );
    await tester.pumpAndSettle();

    expect(find.text("Undefined name 'naoexiste'."), findsOneWidget);
    expect(
      find.text('lib/aritmetica.dart:2  →  aritmetica.cpp:1'),
      findsOneWidget,
    );
    expect(find.text('Unused import.'), findsOneWidget);
    expect(find.text('lib/aritmetica.dart:1'), findsOneWidget);
  });

  testWidgets(
    'a diagnostic with a C++ origin is clickable and navigates there',
    (tester) async {
      DartDiagnostic? selected;
      await tester.pumpWidget(
        _host(
          Future.value(const [errorWithOrigin]),
          onDiagnosticSelected: (diagnostic) => selected = diagnostic,
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text("Undefined name 'naoexiste'."));
      await tester.pump();

      expect(selected, errorWithOrigin);
    },
  );

  testWidgets('a diagnostic without a resolvable origin is not clickable', (
    tester,
  ) async {
    DartDiagnostic? selected;
    await tester.pumpWidget(
      _host(
        Future.value(const [warningWithoutOrigin]),
        onDiagnosticSelected: (diagnostic) => selected = diagnostic,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Unused import.'));
    await tester.pump();

    expect(selected, isNull);
  });
}

Widget _host(
  Future<List<DartDiagnostic>>? diagnostics, {
  ValueChanged<DartDiagnostic>? onDiagnosticSelected,
}) {
  return MaterialApp(
    home: Scaffold(
      body: DiagnosticsView(
        diagnostics: diagnostics,
        onDiagnosticSelected: onDiagnosticSelected ?? (_) {},
      ),
    ),
  );
}
