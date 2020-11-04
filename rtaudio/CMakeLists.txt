cmake_minimum_required(VERSION 3.10)
project(crtaudio C CXX)

set(CMAKE_CXX_STANDARD 11)

set(RTAUDIO_BUILD_TESTING OFF)

add_subdirectory(rtaudio EXCLUDE_FROM_ALL)
include_directories(rtaudio)

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wall -Wextra")
set(CMAKE_CXX_FLAGS_RELEASE "${CMAKE_CXX_FLAGS_RELEASE}")
set(CMAKE_CXX_FLAGS_DEBUG "${CMAKE_CXX_FLAGS_DEBUG} -ggdb -g3 -fno-omit-frame-pointer")

add_library(crtaudio rtaudio.cpp)
target_link_libraries(crtaudio rtaudio)
