#include "valor_numerico.hpp"

double lerComoDouble(TagValorNumerico tag, const ValorNumerico& valor) {
    switch (tag) {
        case TagValorNumerico::Inteiro:
            return static_cast<double>(valor.comoInteiro);
        case TagValorNumerico::PontoFlutuante:
            return static_cast<double>(valor.comoPontoFlutuante);
    }
    return 0.0;
}
