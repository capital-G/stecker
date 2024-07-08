cmake_minimum_required(VERSION 3.25)
project(SuperStecker)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_SHARED_MODULE_PREFIX "")

if(APPLE OR WIN32)
    set(CMAKE_SHARED_MODULE_SUFFIX ".scx")
endif()

# from https://github.com/corrosion-rs/corrosion
include(FetchContent)
FetchContent_Declare(
    Corrosion
    GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
    GIT_TAG v0.5 # Optionally specify a commit hash, version tag or branch here
)
FetchContent_MakeAvailable(Corrosion)

corrosion_import_crate(
    MANIFEST_PATH Cargo.toml
    CRATE_TYPES staticlib
    FLAGS "-vv"
    CRATES stecker_sc
)

corrosion_add_cxxbridge(stecker_rs
    CRATE stecker_sc
    FILES lib.rs
)

add_library(SuperStecker MODULE src/SuperStecker.cpp)

target_include_directories(SuperStecker PUBLIC ${SC_SRC_PATH}/include/plugin_interface)
target_include_directories(SuperStecker PUBLIC ${SC_SRC_PATH}/include/common)
target_include_directories(SuperStecker PUBLIC ${SC_SRC_PATH}/common)

target_link_libraries(SuperStecker PUBLIC stecker_rs)

if(APPLE)
    # macOS only
    # as per https://github.com/corrosion-rs/corrosion/issues/190
    # we need to link to the Security and CoreFoundation Frameworks
    # Otherwise we get undefined symbols during build
    target_link_libraries(SuperStecker PRIVATE "-framework SystemConfiguration" "-framework CoreFoundation" "-framework Security")
endif()

if(NOT CMAKE_INSTALL_PREFIX)
  set(CMAKE_INSTALL_PREFIX ${PROJECT_SOURCE_DIR}/install)
endif()

install(TARGETS SuperStecker LIBRARY DESTINATION ${PROJECT_NAME})
install(FILES src/SuperStecker.sc DESTINATION ${PROJECT_NAME}/Classes)
