cmake_minimum_required(VERSION 3.10)
project(A06TemplateMonomorfizacaoLocal CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(a06_template_monomorfizacao_local STATIC
    src/maior.cpp
)

target_include_directories(a06_template_monomorfizacao_local PUBLIC src)
