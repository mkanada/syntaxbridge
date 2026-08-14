#pragma once

class ContaBancaria {
public:
    ContaBancaria(double saldoInicial);
    ContaBancaria();

    void depositar(double valor);
    double saldo() const;
    int totalDeContas() const;

private:
    double saldo_;
    static int totalContas;
};

double testarDeposito(double inicial, double valor);
double testarContaVazia();
int testarContagemDeContas();
