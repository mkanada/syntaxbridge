#pragma once

// `valor() const` só lê; `valor()` sem `const` avança e retorna o próximo
// número — duas operações com semânticas diferentes que só existem como
// "sobrecarga" porque C++ despacha por const-ness do objeto. Dart não tem
// esse eixo de despacho: precisam de dois nomes (ex.: `valorAtual()` e
// `proximoValor()`).
class Contador {
public:
    explicit Contador(int inicial) : valor_(inicial) {}

    int valor() const;
    int valor();

private:
    int valor_;
};
