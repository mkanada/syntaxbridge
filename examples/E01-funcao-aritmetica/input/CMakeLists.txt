cmake_minimum_required(VERSION 3.10)
project(E01FuncaoAritmetica CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(e01_funcao_aritmetica STATIC
    src/aritmetica.cpp
)

target_include_directories(e01_funcao_aritmetica PUBLIC src)
