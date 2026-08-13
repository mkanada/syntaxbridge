#pragma once

#include <cstddef>

// Regra dos Três: construtor de cópia e `operator=` fazem cópia PROFUNDA do
// buffer próprio; o destrutor libera a memória. `BufferProprio a = b;`
// produz dois buffers independentes em C++. Em Dart, atribuição é sempre
// referência — `var a = b;` faria `a` e `b` apontarem para o MESMO objeto,
// e não existe construtor de cópia para interceptar `=`. Não há
// mapeamento de tipo que preserve isso: só código ponte (um método
// `clonar()` explícito, chamado em todo ponto do C++ original que copiava
// implicitamente) mantém a semântica de valor observável.
class BufferProprio {
public:
    explicit BufferProprio(size_t tamanho);
    BufferProprio(const BufferProprio& outro);
    BufferProprio& operator=(const BufferProprio& outro);
    ~BufferProprio();

    void definir(size_t indice, int valor);
    int obter(size_t indice) const;
    size_t tamanho() const { return tamanho_; }

private:
    void liberar();

    int* dados_;
    size_t tamanho_;
};
