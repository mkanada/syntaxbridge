#pragma once

#include "nadador.hpp"
#include "voador.hpp"

// Herda de duas classes com métodos de nomes diferentes: nenhum conflito a
// resolver, então a combinação classe+mixins é a única opção que faz
// sentido oferecer, sem alternativas.
class Pato : public Voador, public Nadador {
public:
    double altitudeMaxima() const override;
    double profundidadeMaxima() const override;
};
