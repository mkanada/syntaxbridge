#include "pato.hpp"

int testarAltitude() {
    PatoDaguaVoador pato;
    pato.subir();
    pato.subir();
    return pato.altitude;
}

int testarProfundidade() {
    PatoDaguaVoador pato;
    pato.mergulhar();
    return pato.profundidade;
}

std::string testarMovimento() {
    PatoDaguaVoador pato;
    return pato.mover();
}
