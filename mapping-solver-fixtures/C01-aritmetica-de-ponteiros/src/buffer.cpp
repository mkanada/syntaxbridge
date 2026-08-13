#include "buffer.hpp"

int somaJanela(const int* dados, int inicio, int tamanho) {
    const int* ponteiro = dados + inicio;
    int soma = 0;
    for (int i = 0; i < tamanho; ++i) {
        soma = soma + *(ponteiro + i);
    }
    return soma;
}
