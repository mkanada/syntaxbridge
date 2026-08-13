#include "plataforma.hpp"

int valorPadrao(const Config& config) {
#ifdef SYNTAX_BRIDGE_PLATAFORMA_A
    return config.modoA;
#else
    return static_cast<int>(config.modoB);
#endif
}
