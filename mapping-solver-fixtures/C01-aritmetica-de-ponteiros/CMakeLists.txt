cmake_minimum_required(VERSION 3.10)
project(C01AritmeticaDePonteiros CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(c01_aritmetica_de_ponteiros STATIC
    src/buffer.cpp
)

target_include_directories(c01_aritmetica_de_ponteiros PUBLIC src)
