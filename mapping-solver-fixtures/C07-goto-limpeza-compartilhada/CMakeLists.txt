cmake_minimum_required(VERSION 3.10)
project(C07GotoLimpezaCompartilhada CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(c07_goto_limpeza_compartilhada STATIC
    src/recurso_duplo.cpp
)

target_include_directories(c07_goto_limpeza_compartilhada PUBLIC src)
