#pragma once

#include <vector>

// Caso base "fácil" de contêiner STL, incluído de propósito para contrastar
// com os casos difíceis (B05, sobre `std::string` como texto vs. bytes):
// `std::vector<int>` usado só com `push_back`/indexação/tamanho mapeia
// direto para `List<int>`, sem decisão nenhuma a fazer.
std::vector<int> dobrar(const std::vector<int>& valores);
