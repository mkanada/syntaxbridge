cmake_minimum_required(VERSION 3.10)
project(A05ConstENaoConstOverload CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a05_const_e_nao_const_overload STATIC
    src/contador.cpp
)

target_include_directories(a05_const_e_nao_const_overload PUBLIC src)
