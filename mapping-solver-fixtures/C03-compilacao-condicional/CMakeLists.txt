cmake_minimum_required(VERSION 3.10)
project(C03CompilacaoCondicional CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(c03_compilacao_condicional STATIC
    src/plataforma.cpp
)

target_include_directories(c03_compilacao_condicional PUBLIC src)

# A escolha de qual ramo compila é do CMake, não do C++ — exatamente o que
# torna o caso difícil: `compile_commands.json` só descreve a unidade de
# compilação DEPOIS de escolhido um ramo. O outro ramo (SYNTAX_BRIDGE_
# PLATAFORMA_A não definida) existe no texto fonte mas nunca passa pelo
# `libclang` desta configuração — ver docs/mapping-solver-cases.md para a
# implicação sobre US-7.
target_compile_definitions(c03_compilacao_condicional PUBLIC SYNTAX_BRIDGE_PLATAFORMA_A)
