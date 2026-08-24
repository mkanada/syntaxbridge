class Guarda {
  static int contadorAberto = 0;

  Guarda() {
    Guarda.contadorAberto = Guarda.contadorAberto + 1;
  }

  Guarda.syntaxBridgeCopyOf(Guarda other) {}
}

int contadorAtual() {
  return Guarda.contadorAberto;
}

void usarGuarda() {
  Guarda();
  try {
    Guarda.contadorAberto = Guarda.contadorAberto + 100;
  } finally {
    Guarda.contadorAberto = Guarda.contadorAberto - 1;
  }
}

int testarGuardaFechaAoSair() {
  int antes = contadorAtual();
  usarGuarda();
  int depois = contadorAtual();
  return depois - antes;
}

int testarExcecaoCapturada() {
  try {
    throw 42;
  } on int catch (codigo) {
    return codigo + 1;
  }
}
