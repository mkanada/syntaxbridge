#pragma once

// `Base` não tem nada de especial sozinha — um `struct` comum, sem base,
// sem herança múltipla. O que a torna interessante só aparece combinando
// `carro.hpp` (que a força a virar `mixin`) com `standalone.cpp` (que a
// instancia diretamente como valor).
class Base {
public:
    int valor = 0;
    void Fazer() {}
};
