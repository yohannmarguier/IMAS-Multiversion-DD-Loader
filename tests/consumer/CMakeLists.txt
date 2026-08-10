# Throwaway consumer project (issue #5). Not part of the main build: it is
# configured on its own, against an *installed* imas-mvdd-loader, to prove
# find_package works the way a downstream CMake project would use it —
# using only installed artifacts, never this repo's source or build tree.

cmake_minimum_required(VERSION 3.20)
project(imas-mvdd-loader-consumer LANGUAGES C)

find_package(imas-mvdd-loader REQUIRED)

add_executable(consumer_smoke main.c)
target_link_libraries(consumer_smoke PRIVATE imas-mvdd-loader::imas-mvdd-loader)
