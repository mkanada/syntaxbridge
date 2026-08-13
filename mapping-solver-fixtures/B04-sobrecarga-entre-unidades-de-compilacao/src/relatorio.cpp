#include "relatorio.hpp"

#include "formatador.hpp"

std::string gerarRelatorio(int contagem, double media) {
    return formatar(contagem) + " / " + formatar(media);
}
