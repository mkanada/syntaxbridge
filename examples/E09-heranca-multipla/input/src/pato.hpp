#pragma once

#include <string>

class Voador {
public:
    int altitude = 0;
    void subir() {
        altitude = altitude + 10;
    }
    std::string mover() const {
        return "voa";
    }
};

class Nadador {
public:
    int profundidade = 0;
    void mergulhar() {
        profundidade = profundidade + 5;
    }
    std::string mover() const {
        return "nada";
    }
};

class PatoDaguaVoador : public Voador, public Nadador {
public:
    std::string mover() const {
        return "voa e nada";
    }
};

int testarAltitude();
int testarProfundidade();
std::string testarMovimento();
