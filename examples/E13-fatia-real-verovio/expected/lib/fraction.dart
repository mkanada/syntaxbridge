class Fraction {
  int _m_numerator = 0;
  int _m_denominator = 0;

  Fraction([int num = 0]) {}

  Fraction.ctor2(int num, int denom) {
    _m_numerator = num;
    if (denom == 0) {
      LogDebug('Denominator cannot be zero.');
      denom = 1;
    }
    _m_denominator = denom;
    Reduce();
  }

  int GetNumerator() {
    return _m_numerator;
  }

  int GetDenominator() {
    return _m_denominator;
  }

  Fraction operator +(Fraction other) {
    int num =
        _m_numerator * other._m_denominator +
        other._m_numerator * _m_denominator;
    int denom = _m_denominator * other._m_denominator;
    return Fraction.ctor2(num, denom);
  }

  Fraction operator -(Fraction other) {
    int num =
        _m_numerator * other._m_denominator -
        other._m_numerator * _m_denominator;
    int denom = _m_denominator * other._m_denominator;
    return Fraction.ctor2(num, denom);
  }

  Fraction operator *(Fraction other) {
    int num = _m_numerator * other._m_numerator;
    int denom = _m_denominator * other._m_denominator;
    return Fraction.ctor2(num, denom);
  }

  bool operator ==(Object other) {
    if (other is Fraction) {
      return _m_numerator * other._m_denominator ==
          other._m_numerator * _m_denominator;
    }
    return false;
  }

  double ToDouble() {
    return _m_numerator.toDouble() / _m_denominator.toDouble();
  }

  void Reduce() {
    if (_m_denominator < 0) {
      _m_numerator = -_m_numerator;
      _m_denominator = -_m_denominator;
    }
    int gcdVal = _m_numerator.gcd(_m_denominator);
    if (gcdVal != 1) {
      _m_numerator = _m_numerator ~/ gcdVal;
      _m_denominator = _m_denominator ~/ gcdVal;
    }
  }

  static (int, int) ReduceStatic(int numerator, int denominator) {
    Fraction fraction = Fraction.ctor2(numerator, denominator);
    numerator = fraction.GetNumerator();
    denominator = fraction.GetDenominator();
    return (numerator, denominator);
  }
}

void LogDebug(String? fmt) {
  fmt;
}
