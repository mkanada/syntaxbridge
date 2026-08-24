#pragma once

#include <string>

template<typename T>
T dobro(T valor) {
    return valor + valor;
}

template<>
std::string dobro<std::string>(std::string valor);

class Caixa {
public:
    template<typename T>
    T pega(const std::string &chave) const;

    template<typename T>
    bool tem(const std::string &chave) const;
};

template<>
std::string Caixa::pega<std::string>(const std::string &chave) const;

template<>
int Caixa::pega<int>(const std::string &chave) const;

template<>
bool Caixa::tem<std::string>(const std::string &chave) const;

int testarDobroInt();
double testarDobroDouble();
std::string testarDobroString();
std::string testarCaixaString();
int testarCaixaInt();
bool testarCaixaTem();

