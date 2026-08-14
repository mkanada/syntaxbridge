#include "generico.hpp"

template<>
std::string dobro<std::string>(std::string valor) {
    return valor + " (dobrado)";
}

int testarDobroInt() {
    return dobro(5);
}

double testarDobroDouble() {
    return dobro(2.5);
}

std::string testarDobroString() {
    return dobro(std::string("oi"));
}
