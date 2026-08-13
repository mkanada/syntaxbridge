#include "monitor.hpp"

bool mesmoMotor(VeiculoTerrestre& terrestre, Barco& barco) {
    Motor* motorPorTerrestre = static_cast<Motor*>(&terrestre);
    Motor* motorPorBarco = static_cast<Motor*>(&barco);
    return motorPorTerrestre == motorPorBarco;
}
