cmake_minimum_required(VERSION 3.10)
project(A08FloatVsDoublePrecisao CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a08_float_vs_double_precisao STATIC
    src/precisao.cpp
)

target_include_directories(a08_float_vs_double_precisao PUBLIC src)
