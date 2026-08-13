#pragma once

#include "barco.hpp"
#include "veiculo_terrestre.hpp"

// `Anfibio` herda `Motor` por herança virtual dos dois caminhos: existe UM
// só subobjeto `Motor`, então `andar()` e `remar()` giram o mesmo contador.
// Lendo só este arquivo isso já é visível — o que não é visível aqui é que
// monitor.hpp (arquivo separado) depende dessa identidade compartilhada
// para funcionar (ver monitor.hpp). Dart não tem herança virtual nem
// endereço de objeto para comparar por identidade de subobjeto — reproduzir
// "um único Motor compartilhado entre duas superclasses" exige composição
// explícita (um campo `Motor` só, referenciado pelas duas partes), não
// tradução direta de `class X : public virtual Motor`.
class Anfibio : public VeiculoTerrestre, public Barco {
public:
    void mover() {
        andar();
        remar();
    }
};
