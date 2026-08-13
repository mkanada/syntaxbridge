#pragma once

class Motor {
public:
    int rotacoes = 0;
    void girar() { rotacoes = rotacoes + 1; }
};
