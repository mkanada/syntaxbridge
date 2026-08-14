String formatarValorInt(int valor) {
  return 'inteiro';
}

String formatarValorDouble(double valor) {
  return 'real';
}

int incrementar(int valor, [int passo = 1]) {
  return valor + passo;
}

String testarFormatoInt() {
  return formatarValorInt(5);
}

String testarFormatoDouble() {
  return formatarValorDouble(5);
}

int testarIncrementoPadrao() {
  return incrementar(10);
}

int testarIncrementoComPasso() {
  return incrementar(10, 5);
}
