cmake_minimum_required(VERSION 3.10)
project(A04InteirosLarguraFixaOverflow CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a04_inteiros_largura_fixa_overflow STATIC
    src/checksum.cpp
)

target_include_directories(a04_inteiros_largura_fixa_overflow PUBLIC src)
