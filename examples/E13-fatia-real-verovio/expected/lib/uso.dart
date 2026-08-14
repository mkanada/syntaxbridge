import 'fraction.dart';

int testarSoma() {
  Fraction a = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:10: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction b = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:11: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction soma = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:12: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  return soma.GetNumerator() * 100 + soma.GetDenominator();
}

int testarSubtracao() {
  Fraction a = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:18: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction b = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:19: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction diferenca = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:20: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  return diferenca.GetNumerator() * 100 + diferenca.GetDenominator();
}

int testarMultiplicacao() {
  Fraction a = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:26: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction b = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:27: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction produto = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:28: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  return produto.GetNumerator() * 100 + produto.GetDenominator();
}

bool testarIgualdade() {
  Fraction a = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:34: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  Fraction b = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:35: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  return _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:36: method call\'s first child was not the expected member-reference cursor',
  );
}

double testarParaDouble() {
  Fraction a = _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:41: VarDecl had 2 initializer-shaped children, expected at most 1',
  );
  return a.ToDouble();
}

int testarReduzirEstatico() {
  int numerador = 4;
  int denominador = 8;
  _syntaxBridgeUnsupported(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/uso.cpp:49: calling a static method from outside its own class isn\'t supported yet',
  );
  return numerador * 100 + denominador;
}

Never _syntaxBridgeUnsupported(String reason) {
  throw UnimplementedError(reason);
}
