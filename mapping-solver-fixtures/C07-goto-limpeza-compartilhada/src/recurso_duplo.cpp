#include "recurso_duplo.hpp"

#include <cstdlib>

int processarComDoisRecursos(bool falharAoAbrirSegundoRecurso) {
    int* recursoA = static_cast<int*>(std::malloc(sizeof(int)));
    if (recursoA == nullptr) {
        return -1;
    }

    int* recursoB = nullptr;
    int resultado = 0;

    if (falharAoAbrirSegundoRecurso) {
        resultado = -2;
        goto limpar_a;
    }

    recursoB = static_cast<int*>(std::malloc(sizeof(int)));
    if (recursoB == nullptr) {
        resultado = -3;
        goto limpar_a;
    }

    *recursoA = 1;
    *recursoB = 2;
    resultado = *recursoA + *recursoB;

    std::free(recursoB);

limpar_a:
    std::free(recursoA);
    return resultado;
}
