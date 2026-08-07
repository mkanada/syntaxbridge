cmake_minimum_required(VERSION 3.10)
project(SampleGeometryApp CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_executable(sample_app
    src/main.cpp
    src/core/shape.cpp
    src/core/circle.cpp
    src/core/rectangle.cpp
    src/utils/string_utils.cpp
    src/utils/math_utils.cpp
    src/io/logger.cpp
)

target_include_directories(sample_app PRIVATE src)
