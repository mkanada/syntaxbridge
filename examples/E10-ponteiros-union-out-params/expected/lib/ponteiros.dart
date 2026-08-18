void trocar(
  SyntaxBridgeOpaque /* unsupported: int * */ a,
  SyntaxBridgeOpaque /* unsupported: int * */ b,
) {
  // TODO(syntax-bridge): unsupported parameter type: int * (parameter `a`)
  throw UnimplementedError(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E10-ponteiros-union-out-params/input/src/ponteiros.cpp:3: unsupported parameter type: int * (parameter `a`)',
  );
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

final class SyntaxBridgeOpaque {
  const SyntaxBridgeOpaque();
}
