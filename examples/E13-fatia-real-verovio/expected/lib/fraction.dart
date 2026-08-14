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

  bool operator ==(Fraction other) {
    return _m_numerator * other._m_denominator ==
        other._m_numerator * _m_denominator;
  }

  double ToDouble() {
    return _syntaxBridgeUnsupported(
          '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/fraction.cpp:61: unsupported expression cursor kind 124',
        ) /
        _m_denominator.toDouble();
  }

  void Reduce() {
    // TODO(syntax-bridge): unsupported statement cursor kind 115
    throw UnimplementedError(
      '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/fraction.cpp:72: unsupported statement cursor kind 115',
    );
  }

  static void Reduce(int numerator, int denominator) {
    Fraction fraction = Fraction.ctor2(numerator, denominator);
    numerator = fraction.GetNumerator();
    denominator = fraction.GetDenominator();
  }
}

void LogDebug(dynamic /* unsupported: const char * */ fmt) {
  // TODO(syntax-bridge): unsupported parameter type: const char * (parameter `fmt`)
  throw UnimplementedError(
    '/home/mauricio/rust_projects/syntax-bridge/examples/E13-fatia-real-verovio/input/src/fraction.cpp:20: unsupported parameter type: const char * (parameter `fmt`)',
  );
}

Never _syntaxBridgeUnsupported(String reason) {
  throw UnimplementedError(reason);
}
