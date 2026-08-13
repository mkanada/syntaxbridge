#pragma once

#include <cstdint>

enum class TagValorNumerico {
    Inteiro,
    PontoFlutuante,
};

// Dart não tem `union`: os dois campos nunca existem como memória
// sobreposta em Dart, então a única forma de preservar "só um dos dois está
// válido por vez" é código ponte — uma classe com uma tag mais um campo por
// alternativa (ou um wrapper sobre bytes brutos, se a sobreposição binária
// em si importar).
union ValorNumerico {
    int32_t comoInteiro;
    float comoPontoFlutuante;
};

double lerComoDouble(TagValorNumerico tag, const ValorNumerico& valor);
