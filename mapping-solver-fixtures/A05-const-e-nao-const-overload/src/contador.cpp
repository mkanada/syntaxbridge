#include "contador.hpp"

int Contador::valor() const {
    return valor_;
}

int Contador::valor() {
    valor_ = valor_ + 1;
    return valor_;
}
