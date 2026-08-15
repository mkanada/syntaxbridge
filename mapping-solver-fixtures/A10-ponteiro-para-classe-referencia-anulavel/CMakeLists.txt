cmake_minimum_required(VERSION 3.10)
project(A10PonteiroParaClasseReferenciaAnulavel CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a10_ponteiro_para_classe_referencia_anulavel STATIC
    src/editor.cpp
)

target_include_directories(a10_ponteiro_para_classe_referencia_anulavel PUBLIC src)
