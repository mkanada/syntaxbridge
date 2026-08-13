#include "combinado.hpp"

std::string Combinado::nome() const {
    return BaseA::nome() + "+" + BaseB::nome();
}
