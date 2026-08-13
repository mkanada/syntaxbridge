#include "buffer_proprio.hpp"

#include <algorithm>

BufferProprio::BufferProprio(size_t tamanho) : dados_(new int[tamanho]()), tamanho_(tamanho) {}

BufferProprio::BufferProprio(const BufferProprio& outro)
    : dados_(new int[outro.tamanho_]), tamanho_(outro.tamanho_) {
    std::copy(outro.dados_, outro.dados_ + outro.tamanho_, dados_);
}

BufferProprio& BufferProprio::operator=(const BufferProprio& outro) {
    if (this == &outro) {
        return *this;
    }
    liberar();
    dados_ = new int[outro.tamanho_];
    tamanho_ = outro.tamanho_;
    std::copy(outro.dados_, outro.dados_ + outro.tamanho_, dados_);
    return *this;
}

BufferProprio::~BufferProprio() {
    liberar();
}

void BufferProprio::liberar() {
    delete[] dados_;
}

void BufferProprio::definir(size_t indice, int valor) {
    dados_[indice] = valor;
}

int BufferProprio::obter(size_t indice) const {
    return dados_[indice];
}
