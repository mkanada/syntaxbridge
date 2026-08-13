#pragma once

#include <cstddef>
#include <string>

// Uso 2, em outro arquivo: o mesmo tipo `std::string` usado por
// codificarCabecalho é tratado aqui como buffer binário opaco —
// `payload.data()` e `payload.size()` acessando bytes crus, sem qualquer
// pressuposto de que o conteúdo é texto imprimível.
size_t enviarBytes(const std::string& payload);
