#pragma once

class Voador {
public:
    virtual ~Voador() = default;
    virtual double altitudeMaxima() const = 0;
};
