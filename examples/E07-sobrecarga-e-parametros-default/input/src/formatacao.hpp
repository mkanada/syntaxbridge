#pragma once

#include <string>

std::string formatarValor(int valor);
std::string formatarValor(double valor);

int incrementar(int valor, int passo = 1);

std::string testarFormatoInt();
std::string testarFormatoDouble();
int testarIncrementoPadrao();
int testarIncrementoComPasso();
