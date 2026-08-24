import 'dart:typed_data';
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

int soma(Uint8List? buf, int len) {
  int total = 0;
  SyntaxBridgeByteCursor p = SyntaxBridgeByteCursor(buf!);
  while (len >= 4) {
    total = total + (p[0] + p[1] + p[2] + p[3]);
    p = p + 4;
    len = len - 4;
  }
  while (len-- != 0) {
    total = total + (p++).value;
  }
  return total;
}

void zera(Uint8List? buf, int len) {
  for (int i = 0; i < len; i++) {
    buf![i] = 0;
  }
}
