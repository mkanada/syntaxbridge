#pragma once

#include <string>

// As duas declarações moram juntas aqui, mas cada uma é DEFINIDA em uma
// unidade de compilação diferente (formatador_int.cpp, formatador_double.cpp)
// — a renomeação que a sobrecarga por tipo exige (US-7/E07) precisa
// propagar para cada call site em relatorio.cpp, um QUARTO arquivo. Nenhum
// arquivo sozinho mostra "quem chama o quê": só combinando os quatro dá
// para saber que `formatar(int)` deve virar `formatarInt` e todo call site
// em relatorio.cpp precisa ser reescrito junto.
std::string formatar(int valor);
std::string formatar(double valor);
