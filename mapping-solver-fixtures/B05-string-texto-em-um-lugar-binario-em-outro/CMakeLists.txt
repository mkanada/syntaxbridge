cmake_minimum_required(VERSION 3.10)
project(B05StringTextoEmUmLugarBinarioEmOutro CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b05_string_texto_em_um_lugar_binario_em_outro STATIC
    src/saudacao.cpp
    src/transporte.cpp
)

target_include_directories(b05_string_texto_em_um_lugar_binario_em_outro PUBLIC src)
