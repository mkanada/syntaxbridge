import 'syntax_bridge_support.dart';

(int, int) trocar(int a, int b) {
  int temp = a;
  a = b;
  b = temp;
  return (a, b);
}

double lerComoFlutuante(
  SyntaxBridgeOpaque /* unsupported: union ValorBruto */ valor,
) {
  // TODO(syntax-bridge): unsupported parameter type: union ValorBruto (parameter `valor`)
  throw UnimplementedError(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E10-ponteiros-union-out-params/input/src/ponteiros.cpp:9: unsupported parameter type: union ValorBruto (parameter `valor`)',
  );
}

int somarSemPonteiro(int a, int b) {
  return a + b;
}
