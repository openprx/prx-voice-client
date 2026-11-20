# Static build of the qwentts.cpp core and its ggml, for prxvc-qwentts-sys.
#
# Replaces upstream's top-level CMakeLists.txt, which also builds the CLI tools,
# the HTTP server and the tests (their sources are not vendored). The compile
# definitions below are the ones upstream's `qwen-core` target uses. build.rs
# links the resulting archives into one dynamic library that exports only the
# `qt_*` C API.
cmake_minimum_required(VERSION 3.14...3.28)
project(prxvc_qwentts LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

set(QWEN_SRC "${CMAKE_CURRENT_SOURCE_DIR}/../vendor/qwentts.cpp" CACHE PATH "vendored qwentts.cpp tree")
set(QWEN_VERSION_STRING "unknown" CACHE STRING "value returned by qt_version()")

# Everything is hidden by default; the dynamic library's exported-symbols list is
# the second guard.
set(CMAKE_C_VISIBILITY_PRESET hidden)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)

# ggml: static, CPU + Metal (+ Accelerate BLAS as measured), shaders embedded in the
# library so no .metallib sidecar is needed. GGML_NATIVE is off so the shipped binary
# does not depend on the build host's CPU features.
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(GGML_METAL ON CACHE BOOL "" FORCE)
set(GGML_METAL_EMBED_LIBRARY ON CACHE BOOL "" FORCE)
set(GGML_BLAS ON CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
set(GGML_LLAMAFILE OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)

# Audio tokenizer tensor names can exceed ggml's default GGML_MAX_NAME of 64.
add_compile_definitions(GGML_MAX_NAME=128)
add_compile_definitions(_FORTIFY_SOURCE=2)

add_subdirectory("${QWEN_SRC}/ggml" ggml)

file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/version.h"
     "#pragma once\n#define QWEN_VERSION \"${QWEN_VERSION_STRING}\"\n")

find_package(Threads REQUIRED)
add_library(qwen-core STATIC
    "${QWEN_SRC}/src/qwen.cpp"
    "${QWEN_SRC}/src/pipeline-tts.cpp"
    "${QWEN_SRC}/src/pipeline-codec.cpp"
)
target_compile_definitions(qwen-core PUBLIC QWEN_STATIC)
target_include_directories(qwen-core PRIVATE
    "${QWEN_SRC}/src" "${QWEN_SRC}" "${CMAKE_CURRENT_BINARY_DIR}")
target_include_directories(qwen-core SYSTEM PRIVATE "${QWEN_SRC}/ggml/include")
target_compile_options(qwen-core PRIVATE -w)
target_link_libraries(qwen-core PRIVATE ggml Threads::Threads)
