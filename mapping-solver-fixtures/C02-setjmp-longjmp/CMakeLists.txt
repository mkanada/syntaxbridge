cmake_minimum_required(VERSION 3.10)
project(C02SetjmpLongjmp CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(c02_setjmp_longjmp STATIC
    src/recuperacao.cpp
)

target_include_directories(c02_setjmp_longjmp PUBLIC src)
