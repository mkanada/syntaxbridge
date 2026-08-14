#pragma once

struct Ponto3D {
    double x;
    double y;
    double z;
};

inline double normaAoQuadrado(Ponto3D p) {
    return p.x * p.x + p.y * p.y + p.z * p.z;
}
