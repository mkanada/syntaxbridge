#pragma once

#include <string>

// `codificarCabecalho` devolve um `std::string` que, olhando só este
// arquivo e saudacao.cpp, parece texto puro (concatenação, sem indexação
// por byte) — um mapeamento direto para `String` pareceria correto. Só
// transporte.cpp revela que o mesmo valor é tratado como payload binário
// opaco em outro lugar do programa. A decisão `String` vs. `Uint8List`
// (item 12 do checklist de US-7) depende de olhar todos os usos, não só
// este arquivo.
std::string codificarCabecalho(const std::string& nome);
