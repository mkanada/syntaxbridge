#include "escala.hpp"

Ponto3D escalar(Ponto3D p, double fator) {
    Ponto3D resultado;
    resultado.x = p.x * fator;
    resultado.y = p.y * fator;
    resultado.z = p.z * fator;
    return resultado;
}

double testarNormaEscalada() {
    Ponto3D p;
    p.x = 2.0;
    p.y = 0.0;
    p.z = 0.0;
    Ponto3D escalado = escalar(p, 3.0);
    return normaAoQuadrado(escalado);
}
