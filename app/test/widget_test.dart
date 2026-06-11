import 'package:flutter_test/flutter_test.dart';

import 'package:syntax_bridge/main.dart';

void main() {
  testWidgets('shows Rust greeting', (WidgetTester tester) async {
    await tester.pumpWidget(
      const MyApp(
        rustMessageOverride: 'Hello, Syntax Bridge!',
        diagnosticLinesOverride: [
          'Checking diagnostics pipeline...ok',
          'Checking SQLite...ok',
          'Checking Tree-sitter C++...ok',
        ],
      ),
    );

    expect(
      find.text('Flutter chamando Rust via flutter_rust_bridge'),
      findsOneWidget,
    );
    expect(find.text('Hello, Syntax Bridge!'), findsOneWidget);
    expect(find.text('Startup diagnostics'), findsOneWidget);
    expect(find.text('Checking diagnostics pipeline...ok'), findsOneWidget);
    expect(find.text('Checking SQLite...ok'), findsOneWidget);
    expect(find.text('Checking Tree-sitter C++...ok'), findsOneWidget);
  });
}
