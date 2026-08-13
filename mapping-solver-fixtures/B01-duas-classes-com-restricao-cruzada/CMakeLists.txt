cmake_minimum_required(VERSION 3.10)
project(B01DuasClassesComRestricaoCruzada CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b01_duas_classes_com_restricao_cruzada STATIC
    src/atualizador_de_posicao.cpp
)

target_include_directories(b01_duas_classes_com_restricao_cruzada PUBLIC src)
