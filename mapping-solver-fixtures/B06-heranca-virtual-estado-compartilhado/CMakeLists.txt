cmake_minimum_required(VERSION 3.10)
project(B06HerancaVirtualEstadoCompartilhado CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b06_heranca_virtual_estado_compartilhado STATIC
    src/anfibio.cpp
    src/monitor.cpp
)

target_include_directories(b06_heranca_virtual_estado_compartilhado PUBLIC src)
