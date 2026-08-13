cmake_minimum_required(VERSION 3.10)
project(B02DiamanteComMetodosConflitantes CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b02_diamante_com_metodos_conflitantes STATIC
    src/combinado.cpp
)

target_include_directories(b02_diamante_com_metodos_conflitantes PUBLIC src)
