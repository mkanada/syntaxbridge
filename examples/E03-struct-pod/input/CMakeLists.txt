cmake_minimum_required(VERSION 3.10)
project(E03StructPod CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(e03_struct_pod STATIC
    src/ponto.cpp
)

target_include_directories(e03_struct_pod PUBLIC src)
