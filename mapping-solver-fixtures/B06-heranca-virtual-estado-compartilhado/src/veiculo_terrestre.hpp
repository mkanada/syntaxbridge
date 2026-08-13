#pragma once

#include "motor.hpp"

class VeiculoTerrestre : public virtual Motor {
public:
    void andar() { girar(); }
};
