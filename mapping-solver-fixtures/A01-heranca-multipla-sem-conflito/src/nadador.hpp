#pragma once

class Nadador {
public:
    virtual ~Nadador() = default;
    virtual double profundidadeMaxima() const = 0;
};
