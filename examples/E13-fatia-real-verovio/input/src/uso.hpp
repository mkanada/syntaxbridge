#pragma once

// Not extracted from Verovio — declarations for uso.cpp's own driver
// functions, the same role every E01-E12 example's own header plays for its
// free `testarX()` functions. Needed by the oracle harness
// (`tests/conversion_examples.rs`'s `run_cpp_oracle`), which `#include`s
// every header under `input/src` to see these declarations — `uso.cpp` was
// the only fixture in the corpus missing this file, an oversight invisible
// until this degrau's earlier gaps (E13's own findings) stopped blocking
// the oracle stage from ever being reached.

int testarSoma();
int testarSubtracao();
int testarMultiplicacao();
bool testarIgualdade();
double testarParaDouble();
int testarReduzirEstatico();
