#include "fabrica.hpp"

Forma *FabricaDeTriangulo() { return new Triangulo(); }

Forma *FabricaDeQuadrado() { return new Quadrado(); }
