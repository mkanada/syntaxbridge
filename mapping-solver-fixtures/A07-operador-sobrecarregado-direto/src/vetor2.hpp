#pragma once

// `operator+` e `operator==` binários, sem estado externo envolvido: caem
// direto no subconjunto de operadores que Dart também sobrecarrega
// (`operator +`, `operator ==`) — mapeamento óbvio, sem alternativas.
class Vetor2 {
public:
    Vetor2(double x, double y) : x_(x), y_(y) {}

    Vetor2 operator+(const Vetor2& outro) const;
    bool operator==(const Vetor2& outro) const;

    double x() const { return x_; }
    double y() const { return y_; }

private:
    double x_;
    double y_;
};
