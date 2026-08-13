#pragma once

// Dart só tem um tipo de ponto flutuante (`double`, 64 bits). Mapear
// `float` (32 bits) direto para `double` muda o resultado observável de
// contas que dependem do arredondamento de 32 bits — visível já dividindo
// 1.0f / 3.0f e comparando com 1.0 / 3.0 no mesmo arquivo.
float dividirFloat(float a, float b);
double dividirDouble(double a, double b);
