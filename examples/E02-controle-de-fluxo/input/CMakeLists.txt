cmake_minimum_required(VERSION 3.10)
project(E02ControleDeFluxo CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(e02_controle_de_fluxo STATIC
    src/controle.cpp
)

target_include_directories(e02_controle_de_fluxo PUBLIC src)
