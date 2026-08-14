#include "ponteiros.hpp"

void trocar(int* a, int* b) {
    int temp = *a;
    *a = *b;
    *b = temp;
}

float lerComoFlutuante(ValorBruto valor) {
    return valor.comoFlutuante;
}

int somarSemPonteiro(int a, int b) {
    return a + b;
}
