String dobroString(String valor) {
  return valor + ' (dobrado)';
}

double dobroDouble(double valor) {
  return valor + valor;
}

int dobroInt(int valor) {
  return valor + valor;
}

int testarDobroInt() {
  return dobroInt(5);
}

double testarDobroDouble() {
  return dobroDouble(2.5);
}

String testarDobroString() {
  return dobroString('oi');
}
