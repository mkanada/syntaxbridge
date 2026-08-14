#pragma once

#include <string>

template<typename T>
T dobro(T valor) {
    return valor + valor;
}

template<>
std::string dobro<std::string>(std::string valor);

int testarDobroInt();
double testarDobroDouble();
std::string testarDobroString();
