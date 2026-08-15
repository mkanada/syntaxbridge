cmake_minimum_required(VERSION 3.10)
project(B08FabricaDelegada CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b08_fabrica_delegada STATIC
    src/fabrica.cpp
    src/fachada.cpp
)

target_include_directories(b08_fabrica_delegada PUBLIC src)
