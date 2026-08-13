cmake_minimum_required(VERSION 3.10)
project(B03InterfaceImplementadaEmVariosLocais CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b03_interface_implementada_em_varios_locais STATIC
    src/circulo.cpp
    src/quadrado.cpp
    src/triangulo.cpp
)

target_include_directories(b03_interface_implementada_em_varios_locais PUBLIC src)
