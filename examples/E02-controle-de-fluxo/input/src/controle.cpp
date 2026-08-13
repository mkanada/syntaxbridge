#include "controle.hpp"

int divide_inteiro(int a, int b) {
    return a / b;
}

double divide_real(double a, double b) {
    return a / b;
}

int fatorial(int n) {
    if (n <= 1) {
        return 1;
    }
    return n * fatorial(n - 1);
}

int soma_ate(int n) {
    int total = 0;
    for (int i = 1; i <= n; i = i + 1) {
        total = total + i;
    }
    return total;
}

int soma_enquanto(int n) {
    int total = 0;
    int i = 1;
    while (i <= n) {
        total = total + i;
        i = i + 1;
    }
    return total;
}

bool eh_par(int n) {
    if (n % 2 == 0) {
        return true;
    } else {
        return false;
    }
}

int negar(int n) {
    return -n;
}
