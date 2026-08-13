#pragma once

#include "barco.hpp"
#include "veiculo_terrestre.hpp"

// Só faz sentido porque `VeiculoTerrestre` e `Barco`, quando combinados em
// `Anfibio` (anfibio.hpp), compartilham o MESMO subobjeto `Motor` — a
// comparação de endereço abaixo dá `true` só nesse caso. Ler monitor.hpp
// sozinho não mostra por quê; ler anfibio.hpp sozinho não mostra que algo
// depende dessa identidade. A combinação dos dois arquivos é o que prova
// que o bridge de composição precisa preservar "é o mesmo Motor", não só
// "os dois têm um Motor equivalente".
bool mesmoMotor(VeiculoTerrestre& terrestre, Barco& barco);
