cmake_minimum_required(VERSION 3.10)
project(A03UniaoSimples CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a03_uniao_simples STATIC
    src/valor_numerico.cpp
)

target_include_directories(a03_uniao_simples PUBLIC src)
