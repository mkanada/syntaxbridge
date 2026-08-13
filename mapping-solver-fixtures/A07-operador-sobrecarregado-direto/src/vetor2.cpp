#include "vetor2.hpp"

Vetor2 Vetor2::operator+(const Vetor2& outro) const {
    return Vetor2(x_ + outro.x_, y_ + outro.y_);
}

bool Vetor2::operator==(const Vetor2& outro) const {
    return x_ == outro.x_ && y_ == outro.y_;
}
