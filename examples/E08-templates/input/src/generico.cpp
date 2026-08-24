#include "generico.hpp"

template<>
std::string dobro<std::string>(std::string valor) {
    return valor + " (dobrado)";
}

template<>
std::string Caixa::pega<std::string>(const std::string &chave) const {
    return "valor:" + chave;
}

template<>
int Caixa::pega<int>(const std::string &chave) const {
    return static_cast<int>(chave.length());
}

template<>
bool Caixa::tem<std::string>(const std::string &chave) const {
    return chave.length() > 0;
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

std::string testarCaixaString() {
    Caixa c;
    return c.pega<std::string>("teste");
}

int testarCaixaInt() {
    Caixa c;
    return c.pega<int>("cinco");
}

bool testarCaixaTem() {
    Caixa c;
    return c.tem<std::string>("algo");
}

