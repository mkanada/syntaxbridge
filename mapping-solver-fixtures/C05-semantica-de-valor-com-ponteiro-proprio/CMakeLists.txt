cmake_minimum_required(VERSION 3.10)
project(C05SemanticaDeValorComPonteiroProprio CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(c05_semantica_de_valor_com_ponteiro_proprio STATIC
    src/buffer_proprio.cpp
)

target_include_directories(c05_semantica_de_valor_com_ponteiro_proprio PUBLIC src)
