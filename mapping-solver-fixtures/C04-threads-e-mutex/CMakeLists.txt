cmake_minimum_required(VERSION 3.10)
project(C04ThreadsEMutex CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Threads REQUIRED)

add_library(c04_threads_e_mutex STATIC
    src/contador_compartilhado.cpp
)

target_include_directories(c04_threads_e_mutex PUBLIC src)
target_link_libraries(c04_threads_e_mutex PUBLIC Threads::Threads)
