#pragma once

#include "motor.hpp"

class Barco : public virtual Motor {
public:
    void remar() { girar(); }
};
