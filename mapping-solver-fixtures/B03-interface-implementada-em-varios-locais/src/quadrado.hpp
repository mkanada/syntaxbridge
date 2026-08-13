#pragma once

#include "desenhavel.hpp"

class Quadrado : public Desenhavel {
public:
    explicit Quadrado(double lado) : lado_(lado) {}
    void desenhar() const override;

private:
    double lado_;
};
