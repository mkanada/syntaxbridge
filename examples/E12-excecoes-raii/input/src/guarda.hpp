#pragma once

class Guarda {
public:
    Guarda();
    ~Guarda();
    static int contadorAberto;
};

int contadorAtual();
void usarGuarda();
int testarGuardaFechaAoSair();
int testarExcecaoCapturada();
