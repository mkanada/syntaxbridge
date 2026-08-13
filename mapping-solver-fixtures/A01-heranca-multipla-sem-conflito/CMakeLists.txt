cmake_minimum_required(VERSION 3.10)
project(A01HerancaMultiplaSemConflito CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a01_heranca_multipla_sem_conflito STATIC
    src/pato.cpp
)

target_include_directories(a01_heranca_multipla_sem_conflito PUBLIC src)
