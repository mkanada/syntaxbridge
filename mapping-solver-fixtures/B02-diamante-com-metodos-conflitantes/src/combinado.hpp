#pragma once

#include <string>

#include "base_a.hpp"
#include "base_b.hpp"

// `Combinado` herda `nome()` de dois lugares diferentes; em Dart, `with
// BaseA, BaseB` não compila (dois mixins declarando o mesmo método sem
// resolução automática). Em Dart isso é resolvido explicitamente (`with
// BaseA, BaseB` escolhe o último, ou o composto sobrescreve `nome()` — como
// C++ já faz aqui) — mas nenhuma opção que ofereça "os dois como mixins,
// sem sobrescrita" é viável, e só dá para saber disso olhando as duas bases
// junto com esta classe.
class Combinado : public BaseA, public BaseB {
public:
    std::string nome() const;
};
