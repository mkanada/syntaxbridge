#include "conta.hpp"

int ContaBancaria::totalContas = 0;

ContaBancaria::ContaBancaria(double saldoInicial) {
    saldo_ = saldoInicial;
    totalContas = totalContas + 1;
}

ContaBancaria::ContaBancaria() {
    saldo_ = 0.0;
    totalContas = totalContas + 1;
}

void ContaBancaria::depositar(double valor) {
    saldo_ = saldo_ + valor;
}

double ContaBancaria::saldo() const {
    return saldo_;
}

int ContaBancaria::totalDeContas() const {
    return totalContas;
}

double testarDeposito(double inicial, double valor) {
    ContaBancaria conta(inicial);
    conta.depositar(valor);
    return conta.saldo();
}

double testarContaVazia() {
    ContaBancaria conta;
    return conta.saldo();
}

int testarContagemDeContas() {
    ContaBancaria referencia;
    int antes = referencia.totalDeContas();
    ContaBancaria a(10.0);
    int meio = a.totalDeContas();
    ContaBancaria b(20.0);
    int depois = b.totalDeContas();
    return (meio - antes) + (depois - meio);
}
