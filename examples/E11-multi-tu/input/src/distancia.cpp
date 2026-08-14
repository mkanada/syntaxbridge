#include "distancia.hpp"
#include "comum.hpp"

double testarNormaOrigem() {
    Ponto3D p;
    p.x = 3.0;
    p.y = 4.0;
    p.z = 0.0;
    return normaAoQuadrado(p);
}
