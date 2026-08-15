cmake_minimum_required(VERSION 3.10)
project(B09MixinForcadoVsInstanciacaoDireta CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(b09_mixin_forcado_vs_instanciacao_direta STATIC
    src/carro.cpp
    src/standalone.cpp
)

target_include_directories(b09_mixin_forcado_vs_instanciacao_direta PUBLIC src)
