#pragma once

// `Nota` é um tipo do próprio projeto — o catálogo de tipos já o representa
// por inteiro. `Nota*` em `Editor` é o idioma comum de "referência opcional
// a um único objeto": atribuído a partir de um objeto real, comparado com
// `nullptr`, nunca indexado ou incrementado. C++ garante estaticamente que
// o ponteiro só pode ser nulo ou um `Nota` (ou subtipo) — o conjunto de
// valores possíveis é finito por construção, o mesmo que já torna a
// polimorfia de referência única do Dart correta sem enumerar subtipos.
// Contraste com C01 (`mapping-solver-fixtures/C01-aritmetica-de-ponteiros/`):
// lá o ponteiro aponta para `int` e é deslocado por aritmética — nenhuma
// garantia estática afasta o uso como buffer, e só código ponte (`dart:ffi`)
// resolve.
class Nota {
public:
    int altura;
};

class Editor {
public:
    void Definir(Nota *nota);
    Nota *Atual() const;

private:
    Nota *m_atual;
};
