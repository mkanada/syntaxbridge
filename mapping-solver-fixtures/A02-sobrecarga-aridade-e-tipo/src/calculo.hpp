#pragma once

#include <string>

// area(lado) e area(largura, altura): sobrecarga por número de parâmetros —
// Dart resolve isso com um parâmetro opcional, sem precisar renomear nada.
double area(double lado);
double area(double largura, double altura);

// paraTexto(int) e paraTexto(double): mesma aridade, tipos diferentes — Dart
// não despacha por tipo estático, então as duas precisam de nomes distintos.
std::string paraTexto(int valor);
std::string paraTexto(double valor);
