#include "animais.hpp"

Animal::~Animal() {}

std::string Animal::apresentar() const {
    return "Eu digo: " + falar();
}

std::string Cachorro::falar() const {
    return "Au au";
}

std::string Gato::falar() const {
    return "Miau";
}

std::string apresentarAnimal(const Animal& animal) {
    return animal.apresentar();
}

std::string testarCachorro() {
    Cachorro c;
    return apresentarAnimal(c);
}

std::string testarGato() {
    Gato g;
    return apresentarAnimal(g);
}
