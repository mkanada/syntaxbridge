cmake_minimum_required(VERSION 3.10)
project(E04ClasseComEncapsulamento CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(e04_classe_com_encapsulamento STATIC
    src/conta.cpp
)

target_include_directories(e04_classe_com_encapsulamento PUBLIC src)
