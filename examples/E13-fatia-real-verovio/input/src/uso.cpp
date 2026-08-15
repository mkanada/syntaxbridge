// Not extracted from Verovio — driver functions written for this fixture,
// same role every E01-E12 example's own free `testarX()` functions play:
// exercising the extracted class (fraction.hpp/fraction.cpp) so the golden
// and the behavioral oracle have something to compare.

#include "fraction.hpp"
#include "uso.hpp"

int testarSoma()
{
    vrv::Fraction a(1, 2);
    vrv::Fraction b(1, 3);
    vrv::Fraction soma = a + b;
    return soma.GetNumerator() * 100 + soma.GetDenominator();
}

int testarSubtracao()
{
    vrv::Fraction a(3, 4);
    vrv::Fraction b(1, 4);
    vrv::Fraction diferenca = a - b;
    return diferenca.GetNumerator() * 100 + diferenca.GetDenominator();
}

int testarMultiplicacao()
{
    vrv::Fraction a(2, 3);
    vrv::Fraction b(3, 4);
    vrv::Fraction produto = a * b;
    return produto.GetNumerator() * 100 + produto.GetDenominator();
}

bool testarIgualdade()
{
    vrv::Fraction a(2, 4);
    vrv::Fraction b(1, 2);
    return a == b;
}

double testarParaDouble()
{
    vrv::Fraction a(3, 4);
    return a.ToDouble();
}

int testarReduzirEstatico()
{
    int numerador = 4;
    int denominador = 8;
    vrv::Fraction::Reduce(numerador, denominador);
    return numerador * 100 + denominador;
}
