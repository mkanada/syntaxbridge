void trocar(
  dynamic /* unsupported: int * */ a,
  dynamic /* unsupported: int * */ b,
) {
  // TODO(syntax-bridge): unsupported parameter type: int * (parameter `a`)
  throw UnimplementedError(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E10-ponteiros-union-out-params/input/src/ponteiros.cpp:3: unsupported parameter type: int * (parameter `a`)',
  );
}

dynamic /* unsupported: float */ lerComoFlutuante(
  dynamic /* unsupported: union ValorBruto */ valor,
) {
  // TODO(syntax-bridge): unsupported return type: float
  throw UnimplementedError(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E10-ponteiros-union-out-params/input/src/ponteiros.cpp:9: unsupported return type: float',
  );
}

int somarSemPonteiro(int a, int b) {
  return a + b;
}
