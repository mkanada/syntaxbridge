#pragma once

// Mesma forma do B07: `Forma` tem duas subclasses no projeto, então CHA
// (subir a hierarquia) sozinho nunca estreita além de {Forma, Triangulo,
// Quadrado} para nenhum `Forma*` do projeto — mesmo quando `Quadrado` nunca
// é construído em lugar nenhum deste fixture.
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
