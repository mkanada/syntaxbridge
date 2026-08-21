class ContaBancaria {
  double _saldo = 0;
  static int _totalContas = 0;

  ContaBancaria(double saldoInicial) {
    _saldo = saldoInicial;
    ContaBancaria._totalContas = ContaBancaria._totalContas + 1;
  }

  ContaBancaria.ctor2() {
    _saldo = 0;
    ContaBancaria._totalContas = ContaBancaria._totalContas + 1;
  }

  void depositar(double valor) {
    _saldo = _saldo + valor;
  }

  double saldo() {
    return _saldo;
  }

  int totalDeContas() {
    return ContaBancaria._totalContas;
  }
}

double testarDeposito(double inicial, double valor) {
  ContaBancaria conta = ContaBancaria(inicial);
  conta.depositar(valor);
  return conta.saldo();
}

double testarContaVazia() {
  ContaBancaria conta = ContaBancaria.ctor2();
  return conta.saldo();
}

int testarContagemDeContas() {
  ContaBancaria referencia = ContaBancaria.ctor2();
  int antes = referencia.totalDeContas();
  ContaBancaria a = ContaBancaria(10);
  int meio = a.totalDeContas();
  ContaBancaria b = ContaBancaria(20);
  int depois = b.totalDeContas();
  return meio - antes + (depois - meio);
}
