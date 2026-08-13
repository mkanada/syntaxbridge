cmake_minimum_required(VERSION 3.10)
project(A02SobrecargaAridadeETipo CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a02_sobrecarga_aridade_e_tipo STATIC
    src/calculo.cpp
)

target_include_directories(a02_sobrecarga_aridade_e_tipo PUBLIC src)
