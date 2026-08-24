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

unsigned soma(const unsigned char *buf, int len) {
    unsigned total = 0;
    const unsigned char *p = buf;
    while (len >= 4) {
        total += p[0] + p[1] + p[2] + p[3];
        p = p + 4;
        len -= 4;
    }
    while (len--) { total += *p++; }
    return total;
}

void zera(unsigned char *buf, int len) {
    for (int i = 0; i < len; i++) {
        buf[i] = 0;
    }
}
