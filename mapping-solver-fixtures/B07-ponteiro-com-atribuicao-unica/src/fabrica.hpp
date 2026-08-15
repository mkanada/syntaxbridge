#pragma once

#include "forma.hpp"

// Cada função devolve `Forma*`, mas cada uma constrói sempre a mesma
// subclasse concreta — nunca a outra. CHA (subir a hierarquia de `Forma`)
// não enxerga essa diferença: as duas assinaturas são idênticas.
Forma *FabricaDeTriangulo();
Forma *FabricaDeQuadrado();
