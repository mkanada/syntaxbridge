#include "triangulo.hpp"

#include <iostream>

void Triangulo::desenhar() const {
    std::cout << descricaoPadrao() << " base=" << base_ << " altura=" << altura_ << "\n";
}
