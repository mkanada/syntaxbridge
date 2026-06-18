import 'package:flutter_test/flutter_test.dart';
import 'package:syntax_bridge/main.dart';

void main() {
  testWidgets('shows connected status when the Rust server is healthy', (
    tester,
  ) async {
    await tester.pumpWidget(
      SyntaxBridgeApp(
        serverClient: _FakeServerClient(
          const ServerStatus(service: 'syntax-bridge-server', status: 'ok'),
        ),
      ),
    );

    await tester.pumpAndSettle();

    expect(find.text('Syntax Bridge'), findsOneWidget);
    expect(find.text('Connected'), findsOneWidget);
    expect(find.text('syntax-bridge-server'), findsOneWidget);
  });
}

class _FakeServerClient implements ServerClient {
  const _FakeServerClient(this.status);

  final ServerStatus status;

  @override
  Future<ServerStatus> health() async => status;
}
