#include "recuperacao.hpp"

#include <csetjmp>

namespace {
std::jmp_buf ambiente;

void arriscado() {
    std::longjmp(ambiente, 1);
}
}  // namespace

int protegido() {
    if (setjmp(ambiente) == 0) {
        arriscado();
        return 0;
    }
    return -1;
}
