#pragma once

struct Ponto {
    double x;
    double y;
};

Ponto criar_ponto(double x, double y);
double soma_coordenadas(Ponto p);
void mover(Ponto p, double dx, double dy);
bool mover_preserva_original(double x, double y, double dx, double dy);
