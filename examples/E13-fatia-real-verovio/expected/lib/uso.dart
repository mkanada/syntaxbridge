import 'fraction.dart';

int testarSoma() {
  Fraction a = Fraction.ctor2(1, 2);
  Fraction b = Fraction.ctor2(1, 3);
  Fraction soma = a + b;
  return soma.GetNumerator() * 100 + soma.GetDenominator();
}

int testarSubtracao() {
  Fraction a = Fraction.ctor2(3, 4);
  Fraction b = Fraction.ctor2(1, 4);
  Fraction diferenca = a - b;
  return diferenca.GetNumerator() * 100 + diferenca.GetDenominator();
}

int testarMultiplicacao() {
  Fraction a = Fraction.ctor2(2, 3);
  Fraction b = Fraction.ctor2(3, 4);
  Fraction produto = a * b;
  return produto.GetNumerator() * 100 + produto.GetDenominator();
}

bool testarIgualdade() {
  Fraction a = Fraction.ctor2(2, 4);
  Fraction b = Fraction.ctor2(1, 2);
  return a == b;
}

double testarParaDouble() {
  Fraction a = Fraction.ctor2(3, 4);
  return a.ToDouble();
}

int testarReduzirEstatico() {
  int numerador = 4;
  int denominador = 8;
  (numerador, denominador) = Fraction.ReduceStatic(numerador, denominador);
  return numerador * 100 + denominador;
}
