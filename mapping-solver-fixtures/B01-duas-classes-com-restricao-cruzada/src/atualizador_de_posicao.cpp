#include "atualizador_de_posicao.hpp"

void AtualizadorDePosicao::empurrar(Ponto3D& alvo, double deslocamentoX) const {
    alvo.x = alvo.x + deslocamentoX;
}
