# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# One CMake project for every Qt program of TUR's desktop
# (docs/architecture/desktop-experience.md, section 15). It is configured and
# built only in CI, in the builder image of ci/builder: see README.md.

cmake_minimum_required(VERSION 3.28)
project(tur-desktop VERSION 0.1.0 DESCRIPTION "TUR desktop programs" LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

option(TUR_WERROR "Treat compiler warnings as errors (CI does)" OFF)
option(TUR_SANITIZE "Build with AddressSanitizer and UndefinedBehaviorSanitizer" OFF)

# Only LGPL modules of Qt: see ALLOWED_IMPORTS. QuickTest and Test are linked
# by test programs only.
find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Qml Quick QuickControls2 QuickTest Test)
qt_standard_project_setup(REQUIRES 6.8)

# Every QML module of the project is built into one tree, which is the import
# path for the tests and for qmllint.
set(QT_QML_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml)

add_compile_options(-Wall -Wextra)
if(TUR_WERROR)
    add_compile_options(-Werror)
endif()
if(TUR_SANITIZE)
    add_compile_options(-fsanitize=address,undefined -fno-omit-frame-pointer)
    add_link_options(-fsanitize=address,undefined)
endif()

# What every test runs under: no display, Qt Quick's software renderer, a
# fixed font resolution, and nothing read from the machine's own settings.
set(TUR_TEST_ENVIRONMENT
    "QT_QPA_PLATFORM=offscreen"
    "QT_QUICK_BACKEND=software"
    "QT_FONT_DPI=96"
    "QT_SCALE_FACTOR=1"
    "QML_IMPORT_PATH=${QT_QML_OUTPUT_DIRECTORY}"
    "XDG_CONFIG_HOME=${CMAKE_BINARY_DIR}/empty-config"
    "XDG_CONFIG_DIRS=${CMAKE_BINARY_DIR}/empty-config"
    "LC_ALL=C.UTF-8"
    "ASAN_OPTIONS=detect_leaks=0"
    "UBSAN_OPTIONS=print_stacktrace=1"
)

# A program that uses the kit's QML modules links their libraries so that the
# modules are found without an import path. The linker must not drop a library
# just because no symbol of it is called.
function(tur_link_qml_modules target)
    target_link_options(${target} PRIVATE -Wl,--no-as-needed)
    target_link_libraries(${target} PRIVATE ${ARGN})
endfunction()

enable_testing()

add_subdirectory(kit)
add_subdirectory(ci/probe)
