#pragma once

union ValorBruto {
    int comoInteiro;
    float comoFlutuante;
};

void trocar(int* a, int* b);
float lerComoFlutuante(ValorBruto valor);
int somarSemPonteiro(int a, int b);
unsigned soma(const unsigned char *buf, int len);
void zera(unsigned char *buf, int len);
