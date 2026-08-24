import 'dart:convert';

class Caixa {
  Caixa();

  Caixa.syntaxBridgeCopyOf(Caixa other) {}

  String pegaString(String chave) {
    return 'valor:' + chave;
  }

  int pegaInt(String chave) {
    return utf8.encode(chave).length;
  }

  bool temString(String chave) {
    return utf8.encode(chave).length > 0;
  }
}

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

String testarCaixaString() {
  Caixa c = Caixa();
  return c.pegaString('teste');
}

int testarCaixaInt() {
  Caixa c = Caixa();
  return c.pegaInt('cinco');
}

bool testarCaixaTem() {
  Caixa c = Caixa();
  return c.temString('algo');
}
