#pragma once

#include <string>

// Lido sozinho, `BaseB` também parece um mixin perfeito para `nome()`. O
// conflito — dois mixins declarando o mesmo método, com corpos diferentes —
// só aparece quando `combinado.hpp` tenta herdar dos dois ao mesmo tempo.
class BaseB {
public:
    virtual ~BaseB() = default;
    virtual std::string nome() const { return "B"; }
};
