#include "base.hpp"

// Declaração de variável em escopo de arquivo — `Base` instanciada como
// valor direto, não através de um ponteiro/referência nem como parte de
// `Carro`. Um `mixin` em Dart nunca pode ser instanciado sozinho; isto é o
// que torna a exigência de `carro.hpp` (Base precisa virar mixin)
// inviável.
Base valorPadrao;
