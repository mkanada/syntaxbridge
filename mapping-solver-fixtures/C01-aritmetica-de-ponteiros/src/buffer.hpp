#pragma once

// `dados + inicio` desloca o ponteiro em unidades de `int`, e o laço avança
// com `*(ponteiro + i)`. Não existe mapeamento de tipo que resolva isso:
// Dart não tem ponteiro nem aritmética de endereço sobre `List`. A única
// forma de manter a conversão possível é código ponte de verdade —
// `dart:ffi` (`Pointer<Int32>`, `.elementAt`) — não uma escolha entre
// opções de classe.
int somaJanela(const int* dados, int inicio, int tamanho);
