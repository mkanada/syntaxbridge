// Slice extracted from Verovio 6.2.0 (src/fraction.cpp) — see fraction.hpp
// for exactly what was kept and why. Every method body below that exists in
// the real file is copied unmodified (`Fraction::Fraction(int, int)`,
// `operator+`, `operator-`, `operator*`, `operator==`, `ToDouble`, the
// private `Reduce()`, the static `Reduce(int&, int&)`).
//
// `LogDebug` is NOT from the real file: Verovio's own logging
// infrastructure is a variadic, printf-style function defined in vrv.cpp
// (itself gated behind a `#if defined(DEBUG)` and pulling in the rest of
// the logging subsystem) — out of scope for a self-contained slice. This
// stub keeps the same call site the real constructor has, with the real
// signature, but does nothing.

#include "fraction.hpp"

#include <numeric>

namespace vrv {

void LogDebug(const char *fmt, ...) { (void)fmt; }

Fraction::Fraction(int num, int denom)
{
    m_numerator = num;
    if (denom == 0) {
        LogDebug("Denominator cannot be zero.");
        denom = 1;
    }
    m_denominator = denom;
    this->Reduce();
}

Fraction Fraction::operator+(const Fraction &other) const
{
    int num = m_numerator * other.m_denominator + other.m_numerator * m_denominator;
    int denom = m_denominator * other.m_denominator;
    return Fraction(num, denom);
}

Fraction Fraction::operator-(const Fraction &other) const
{
    int num = m_numerator * other.m_denominator - other.m_numerator * m_denominator;
    int denom = m_denominator * other.m_denominator;
    return Fraction(num, denom);
}

Fraction Fraction::operator*(const Fraction &other) const
{
    int num = m_numerator * other.m_numerator;
    int denom = m_denominator * other.m_denominator;
    return Fraction(num, denom);
}

bool Fraction::operator==(const Fraction &other) const
{
    return m_numerator * other.m_denominator == other.m_numerator * m_denominator;
}

double Fraction::ToDouble() const
{
    return static_cast<double>(m_numerator) / m_denominator;
}

void Fraction::Reduce()
{
    if (m_denominator < 0) { // Keep the denominator positive
        m_numerator = -m_numerator;
        m_denominator = -m_denominator;
    }
    const int gcdVal = std::gcd(m_numerator, m_denominator);
    if (gcdVal != 1) {
        m_numerator /= gcdVal;
        m_denominator /= gcdVal;
    }
}

void Fraction::Reduce(int &numerator, int &denominator)
{
    Fraction fraction(numerator, denominator);
    numerator = fraction.GetNumerator();
    denominator = fraction.GetDenominator();
}

} // namespace vrv
