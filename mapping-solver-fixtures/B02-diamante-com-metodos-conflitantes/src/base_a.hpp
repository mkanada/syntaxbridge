#pragma once

#include <string>

// Lido sozinho, `BaseA` parece um mixin perfeito para `nome()`.
class BaseA {
public:
    virtual ~BaseA() = default;
    virtual std::string nome() const { return "A"; }
};
