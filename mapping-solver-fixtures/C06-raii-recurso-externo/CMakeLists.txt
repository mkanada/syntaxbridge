cmake_minimum_required(VERSION 3.10)
project(C06RaiiRecursoExterno CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(c06_raii_recurso_externo STATIC
    src/arquivo_texto.cpp
)

target_include_directories(c06_raii_recurso_externo PUBLIC src)
