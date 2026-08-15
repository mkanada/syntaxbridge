#pragma once

// `Forma` tem duas subclasses no projeto inteiro — `Triangulo` e
// `Quadrado`, ambas em `fabrica.cpp`. A hierarquia por si só (CHA) não diz
// qual delas um `Forma*` específico pode de fato assumir: só olhando onde
// cada ponteiro é construído (`fabrica.cpp`) é que fica claro que cada
// função devolve sempre a mesma subclasse, nunca a outra.
class Forma {
public:
    virtual ~Forma() = default;
    virtual int Lados() const { return 0; }
};

class Triangulo : public Forma {
public:
    int Lados() const override { return 3; }
};

class Quadrado : public Forma {
public:
    int Lados() const override { return 4; }
};
