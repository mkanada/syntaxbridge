#pragma once

#include "desenhavel.hpp"

class Triangulo : public Desenhavel {
public:
    Triangulo(double base, double altura) : base_(base), altura_(altura) {}
    void desenhar() const override;

private:
    double base_;
    double altura_;
};
