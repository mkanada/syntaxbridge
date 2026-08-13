#pragma once

#include <cstddef>
#include <cstdint>

// A soma estoura de propósito em uint8_t (módulo 256) — o `int` de 64 bits
// do Dart nunca estoura no mesmo ponto, então um mapeamento ingênuo para
// `int` muda o resultado observável. Precisa de mascaramento explícito
// (`& 0xFF` a cada soma) no Dart gerado, não só troca de tipo.
uint8_t checksum(const uint8_t* dados, size_t tamanho);
