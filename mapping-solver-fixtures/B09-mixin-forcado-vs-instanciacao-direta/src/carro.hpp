#pragma once

#include "base.hpp"

// `Rodas` não compartilha nenhum nome de método com `Base` (`Girar` vs.
// `Fazer`), então a herança múltipla de `Carro` é o caso sem conflito —
// `mapping::options_for(Carro, ...)` escolhe "classe-com-mixins" direto, e
// é essa opção que anexa a `Base` a consequência "vira mixin aplicado via
// `with`".
class Rodas {
public:
    void Girar() {}
};

class Carro : public Base, public Rodas {};
