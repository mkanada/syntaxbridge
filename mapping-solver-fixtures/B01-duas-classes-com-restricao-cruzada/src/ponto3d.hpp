#pragma once

// Lido isoladamente, `Ponto3D` parece um candidato perfeito para uma classe
// Dart imutável (campos `final`, sem métodos que o mutam) — a leitura deste
// arquivo, sozinho, não mostra motivo para recusar essa opção. O motivo só
// aparece em atualizador_de_posicao.hpp.
struct Ponto3D {
    double x;
    double y;
    double z;
};
