#include "formatacao.hpp"

std::string formatarValor(int valor) {
    return "inteiro";
}

std::string formatarValor(double valor) {
    return "real";
}

int incrementar(int valor, int passo) {
    return valor + passo;
}

std::string testarFormatoInt() {
    return formatarValor(5);
}

std::string testarFormatoDouble() {
    return formatarValor(5.0);
}

int testarIncrementoPadrao() {
    return incrementar(10);
}

int testarIncrementoComPasso() {
    return incrementar(10, 5);
}
