#include "calculo.hpp"

double area(double lado) {
    return lado * lado;
}

double area(double largura, double altura) {
    return largura * altura;
}

std::string paraTexto(int valor) {
    return std::to_string(valor);
}

std::string paraTexto(double valor) {
    return std::to_string(valor);
}
