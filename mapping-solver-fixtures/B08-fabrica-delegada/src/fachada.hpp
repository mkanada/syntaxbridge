#pragma once

#include "forma.hpp"

// `Obter` não constrói nada no próprio corpo — só encaminha o que
// `FabricaDeTriangulo` (outro arquivo) devolve. Só combinando os dois
// arquivos (e o grafo de chamadas entre eles) fica claro que `Obter` também
// só devolve `Triangulo`, nunca `Forma` puro nem `Quadrado`.
Forma *Obter();
