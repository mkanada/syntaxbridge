cmake_minimum_required(VERSION 3.10)
project(A09VetorStlTrivial CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a09_vetor_stl_trivial STATIC
    src/lista_simples.cpp
)

target_include_directories(a09_vetor_stl_trivial PUBLIC src)
