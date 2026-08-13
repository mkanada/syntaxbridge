#include "lista_simples.hpp"

std::vector<int> dobrar(const std::vector<int>& valores) {
    std::vector<int> resultado;
    resultado.reserve(valores.size());
    for (int valor : valores) {
        resultado.push_back(valor * 2);
    }
    return resultado;
}
