// Slice extracted from Verovio 6.2.0 (include/vrv/fraction.h), for E13's
// "degrau de realidade". Kept as close to verbatim as the ladder's own scope
// allows: every retained method is copied unmodified. Dropped, and noted in
// NOTES.md, are the pieces that need C++ features no rung has built yet —
// the data_DURATION-coupled constructor and ToDur() (Verovio's own duration
// enum, a domain type, not a general-purpose construct), the implicit-`int`
// conversion constructor template with std::enable_if_t SFINAE, the C++20
// spaceship operator (operator<=>), operator/ and operator% (both only
// differ from the kept operators by also calling LogDebug on a
// divide-by-zero guard — same call this file already needs for the two-int
// constructor, so dropping them trims duplication, not new surface), and
// ToString (the only caller of Verovio's own StringFormat, a variadic
// printf-style helper — variadic C functions are a construct this ladder
// has never exercised).
//
// LogDebug itself is declared with its real Verovio signature so the file
// keeps calling the same logging entry point the original does — see
// fraction.cpp for why its body is a stub, not the real one.

#include <string>

namespace vrv {

void LogDebug(const char *fmt, ...);

class Fraction {

public:
    // Constructors - make them explicit to avoid type conversion
    explicit Fraction(int num = 0) : m_numerator(num), m_denominator(1) {}
    explicit Fraction(int num, int denom);

    /** Addition operator */
    Fraction operator+(const Fraction &other) const;
    /** Subtraction operator */
    Fraction operator-(const Fraction &other) const;
    /** Multiplication operator */
    Fraction operator*(const Fraction &other) const;

    /** Equality operator */
    bool operator==(const Fraction &other) const;

    /** Getters */
    int GetNumerator() const { return m_numerator; }
    int GetDenominator() const { return m_denominator; }

    /** Convert fraction to a double */
    double ToDouble() const;

    //----------------//
    // Static methods //
    //----------------//

    /** Reduce the fraction represented by the two numbers */
    static void Reduce(int &numerator, int &denominator);

private:
    /** Reduce the fraction */
    void Reduce();

private:
    int m_numerator;
    int m_denominator;
};

} // namespace vrv
