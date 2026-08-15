cmake_minimum_required(VERSION 3.10)
project(B07PonteiroComAtribuicaoUnica CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b07_ponteiro_com_atribuicao_unica STATIC
    src/fabrica.cpp
)

target_include_directories(b07_ponteiro_com_atribuicao_unica PUBLIC src)
