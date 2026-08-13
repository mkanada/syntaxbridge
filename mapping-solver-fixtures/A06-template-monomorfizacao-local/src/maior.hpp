#pragma once

// Só duas instanciações, ambas no mesmo arquivo: dá para decidir localmente
// entre genéricos de Dart (`T maior<T extends Comparable>(T a, T b)`) e
// monomorfização (uma função `maiorInt`/`maiorDouble` por instanciação) sem
// precisar olhar o resto do projeto.
template <typename T>
T maior(T a, T b) {
    return a > b ? a : b;
}

int usarComInt(int a, int b);
double usarComDouble(double a, double b);
