#include "arquivo_texto.hpp"

ArquivoTexto::ArquivoTexto(const std::string& caminho) : alca_(std::fopen(caminho.c_str(), "w")) {}

ArquivoTexto::~ArquivoTexto() {
    if (alca_ != nullptr) {
        std::fclose(alca_);
    }
}

void ArquivoTexto::escrever(const std::string& conteudo) {
    if (alca_ != nullptr) {
        std::fwrite(conteudo.data(), 1, conteudo.size(), alca_);
    }
}
