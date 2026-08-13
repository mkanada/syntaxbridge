cmake_minimum_required(VERSION 3.10)
project(A07OperadorSobrecarregadoDireto CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a07_operador_sobrecarregado_direto STATIC
    src/vetor2.cpp
)

target_include_directories(a07_operador_sobrecarregado_direto PUBLIC src)
