#include "ponto.hpp"

Ponto criar_ponto(double x, double y) {
    Ponto p;
    p.x = x;
    p.y = y;
    return p;
}

double soma_coordenadas(Ponto p) {
    return p.x + p.y;
}

void mover(Ponto p, double dx, double dy) {
    p.x = p.x + dx;
    p.y = p.y + dy;
}

bool mover_preserva_original(double x, double y, double dx, double dy) {
    Ponto p = criar_ponto(x, y);
    mover(p, dx, dy);
    return p.x == x && p.y == y;
}
