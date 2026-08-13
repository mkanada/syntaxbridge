#include "protocolo.hpp"

// Uso 1: `codificarCabecalho` como texto puro — concatenação, sem tocar
// bytes individuais.
std::string codificarCabecalho(const std::string& nome) {
    return "Ola, " + nome + "!";
}
