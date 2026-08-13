#pragma once

#include "ponto3d.hpp"

// `empurrar` recebe `Ponto3D&` e escreve em `alvo.x` — isso obriga `Ponto3D`
// a virar uma classe Dart com campos mutáveis. Um solver que decida a opção
// de `Ponto3D` olhando só `ponto3d.hpp` poderia oferecer "classe imutável"
// como válida; só ao considerar `AtualizadorDePosicao` (arquivo separado) é
// que essa opção se revela inviável — exatamente o caso do critério 3/Q9 de
// US-7: "uma opção que tornaria outro tipo não convertível não é
// oferecida".
struct AtualizadorDePosicao {
    void empurrar(Ponto3D& alvo, double deslocamentoX) const;
};
