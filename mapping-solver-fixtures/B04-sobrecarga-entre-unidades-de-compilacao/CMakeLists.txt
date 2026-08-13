cmake_minimum_required(VERSION 3.10)
project(B04SobrecargaEntreUnidadesDeCompilacao CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b04_sobrecarga_entre_unidades_de_compilacao STATIC
    src/formatador_int.cpp
    src/formatador_double.cpp
    src/relatorio.cpp
)

target_include_directories(b04_sobrecarga_entre_unidades_de_compilacao PUBLIC src)
