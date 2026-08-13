#pragma once

#include "desenhavel.hpp"

class Circulo : public Desenhavel {
public:
    explicit Circulo(double raio) : raio_(raio) {}
    void desenhar() const override;

private:
    double raio_;
};
