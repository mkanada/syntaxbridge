#pragma once

#include <string>

class Animal {
public:
    virtual ~Animal();
    virtual std::string falar() const = 0;
    std::string apresentar() const;
};

class Cachorro : public Animal {
public:
    std::string falar() const override;
};

class Gato : public Animal {
public:
    std::string falar() const override;
};

std::string apresentarAnimal(const Animal& animal);
std::string testarCachorro();
std::string testarGato();
