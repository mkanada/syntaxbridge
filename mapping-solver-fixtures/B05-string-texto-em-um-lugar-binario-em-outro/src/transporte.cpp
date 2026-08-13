#include "transporte.hpp"

#include <cstring>

size_t enviarBytes(const std::string& payload) {
    unsigned char destino[256];
    size_t tamanho = payload.size() < sizeof(destino) ? payload.size() : sizeof(destino);
    std::memcpy(destino, payload.data(), tamanho);
    return tamanho;
}
