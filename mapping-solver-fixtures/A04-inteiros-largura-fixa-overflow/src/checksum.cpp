#include "checksum.hpp"

uint8_t checksum(const uint8_t* dados, size_t tamanho) {
    uint8_t soma = 0;
    for (size_t i = 0; i < tamanho; ++i) {
        soma = static_cast<uint8_t>(soma + dados[i]);
    }
    return soma;
}
