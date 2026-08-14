#include "guarda.hpp"

int Guarda::contadorAberto = 0;

Guarda::Guarda() {
    contadorAberto = contadorAberto + 1;
}

Guarda::~Guarda() {
    contadorAberto = contadorAberto - 1;
}

int contadorAtual() {
    return Guarda::contadorAberto;
}

void usarGuarda() {
    Guarda g;
    Guarda::contadorAberto = Guarda::contadorAberto + 100;
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
    } catch (int codigo) {
        return codigo + 1;
    }
}
