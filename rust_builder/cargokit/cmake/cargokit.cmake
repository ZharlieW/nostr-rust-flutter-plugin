SET(cargokit_cmake_root "${CMAKE_CURRENT_LIST_DIR}/..")

# Workaround for https://github.com/dart-lang/pub/issues/4010
get_filename_component(cargokit_cmake_root "${cargokit_cmake_root}" REALPATH)

if(WIN32)
    # REALPATH does not properly resolve symlinks on windows :-/
    file(TO_NATIVE_PATH "${CMAKE_CURRENT_LIST_DIR}/resolve_symlinks.ps1" RESOLVE_SYMLINKS_SCRIPT)
    execute_process(
        COMMAND powershell -ExecutionPolicy Bypass -File "${RESOLVE_SYMLINKS_SCRIPT}" "${cargokit_cmake_root}"
        OUTPUT_VARIABLE resolved_path
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET
        RESULT_VARIABLE resolve_result
    )
    
    # Use resolved path if successful, otherwise fallback
    if(resolve_result EQUAL "0" AND resolved_path)
        file(TO_NATIVE_PATH "${resolved_path}" cargokit_cmake_root)
    else()
        # Fallback: calculate from CMAKE_SOURCE_DIR if available
        if(DEFINED CMAKE_SOURCE_DIR)
            file(TO_NATIVE_PATH "${CMAKE_SOURCE_DIR}/packages/nostr-rust-flutter-plugin/rust_builder/cargokit" fallback_path)
            if(EXISTS "${fallback_path}")
                set(cargokit_cmake_root "${fallback_path}")
            else()
                file(TO_NATIVE_PATH "${cargokit_cmake_root}" cargokit_cmake_root)
            endif()
        else()
            file(TO_NATIVE_PATH "${cargokit_cmake_root}" cargokit_cmake_root)
        endif()
    endif()
endif()

# Arguments
# - target: CMAKE target to which rust library is linked
# - manifest_dir: relative path from current folder to directory containing cargo manifest
# - lib_name: cargo package name
# - any_symbol_name: name of any exported symbol from the library.
#                    used on windows to force linking with library.
function(apply_cargokit target manifest_dir lib_name any_symbol_name)

    set(CARGOKIT_LIB_NAME "${lib_name}")
    set(CARGOKIT_LIB_FULL_NAME "${CMAKE_SHARED_MODULE_PREFIX}${CARGOKIT_LIB_NAME}${CMAKE_SHARED_MODULE_SUFFIX}")
    if (CMAKE_CONFIGURATION_TYPES)
        set(CARGOKIT_OUTPUT_DIR "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>")
        set(OUTPUT_LIB "${CMAKE_CURRENT_BINARY_DIR}/$<CONFIG>/${CARGOKIT_LIB_FULL_NAME}")
    else()
        set(CARGOKIT_OUTPUT_DIR "${CMAKE_CURRENT_BINARY_DIR}")
        set(OUTPUT_LIB "${CMAKE_CURRENT_BINARY_DIR}/${CARGOKIT_LIB_FULL_NAME}")
    endif()
    set(CARGOKIT_TEMP_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargokit_build")

    if (FLUTTER_TARGET_PLATFORM)
        set(CARGOKIT_TARGET_PLATFORM "${FLUTTER_TARGET_PLATFORM}")
    else()
        set(CARGOKIT_TARGET_PLATFORM "windows-x64")
    endif()

    # Handle both absolute and relative paths for manifest_dir
    # Fix symlink path resolution issues on Windows
    if(IS_ABSOLUTE "${manifest_dir}")
        set(RESOLVED_MANIFEST_DIR "${manifest_dir}")
        # If absolute path doesn't exist, it might be from symlink - recalculate
        if(NOT EXISTS "${RESOLVED_MANIFEST_DIR}" AND WIN32 AND DEFINED cargokit_cmake_root)
            get_filename_component(PLUGIN_ROOT "${cargokit_cmake_root}" DIRECTORY)
            get_filename_component(PLUGIN_ROOT "${PLUGIN_ROOT}" DIRECTORY)
            file(TO_NATIVE_PATH "${PLUGIN_ROOT}/rust" RESOLVED_MANIFEST_DIR)
        endif()
    else()
        set(RESOLVED_MANIFEST_DIR "${CMAKE_CURRENT_SOURCE_DIR}/${manifest_dir}")
        # If path doesn't exist, try from project root
        if(NOT EXISTS "${RESOLVED_MANIFEST_DIR}" AND DEFINED CMAKE_SOURCE_DIR)
            file(TO_NATIVE_PATH "${CMAKE_SOURCE_DIR}/packages/nostr-rust-flutter-plugin/rust" alt_path)
            if(EXISTS "${alt_path}")
                set(RESOLVED_MANIFEST_DIR "${alt_path}")
            endif()
        endif()
        # Last resort: calculate from cargokit location
        if(NOT EXISTS "${RESOLVED_MANIFEST_DIR}" AND WIN32 AND DEFINED cargokit_cmake_root)
            get_filename_component(PLUGIN_ROOT "${cargokit_cmake_root}" DIRECTORY)
            get_filename_component(PLUGIN_ROOT "${PLUGIN_ROOT}" DIRECTORY)
            file(TO_NATIVE_PATH "${PLUGIN_ROOT}/rust" RESOLVED_MANIFEST_DIR)
        endif()
    endif()

    # Get FLUTTER_ROOT - critical for Windows builds
    # Other platforms can usually find Flutter via PATH or environment
    if(NOT DEFINED FLUTTER_ROOT)
        if(DEFINED ENV{FLUTTER_ROOT})
            set(FLUTTER_ROOT "$ENV{FLUTTER_ROOT}")
        else()
            # Try to find flutter command in PATH (works on all platforms)
            find_program(FLUTTER_CMD flutter)
            if(FLUTTER_CMD)
                get_filename_component(FLUTTER_CMD "${FLUTTER_CMD}" ABSOLUTE)
                get_filename_component(FLUTTER_ROOT "${FLUTTER_CMD}" DIRECTORY)
                get_filename_component(FLUTTER_ROOT "${FLUTTER_ROOT}" DIRECTORY)
            elseif(WIN32)
                # Windows-specific fallback paths (only on Windows)
                find_program(FLUTTER_CMD flutter 
                    PATHS 
                        "C:/flutter/bin"
                        "$ENV{USERPROFILE}/flutter/bin"
                        "$ENV{LOCALAPPDATA}/flutter/bin"
                    NO_DEFAULT_PATH
                )
                if(FLUTTER_CMD)
                    get_filename_component(FLUTTER_ROOT "${FLUTTER_CMD}" DIRECTORY)
                    get_filename_component(FLUTTER_ROOT "${FLUTTER_ROOT}" DIRECTORY)
                endif()
            endif()
        endif()
    endif()

    # Convert all paths to native format on Windows to handle spaces correctly
    if(WIN32)
        file(TO_NATIVE_PATH "${RESOLVED_MANIFEST_DIR}" NATIVE_MANIFEST_DIR)
        file(TO_NATIVE_PATH "${CARGOKIT_TEMP_DIR}" NATIVE_TEMP_DIR)
        file(TO_NATIVE_PATH "${CARGOKIT_OUTPUT_DIR}" NATIVE_OUTPUT_DIR)
        file(TO_NATIVE_PATH "${CMAKE_SOURCE_DIR}" NATIVE_SOURCE_DIR)
        file(TO_NATIVE_PATH "${FLUTTER_ROOT}" NATIVE_FLUTTER_ROOT)
        file(TO_NATIVE_PATH "${CMAKE_COMMAND}" NATIVE_CMAKE_CMD)
        file(TO_NATIVE_PATH "${CARGOKIT_TEMP_DIR}/tool" NATIVE_TOOL_TEMP_DIR)
    else()
        set(NATIVE_MANIFEST_DIR "${RESOLVED_MANIFEST_DIR}")
        set(NATIVE_TEMP_DIR "${CARGOKIT_TEMP_DIR}")
        set(NATIVE_OUTPUT_DIR "${CARGOKIT_OUTPUT_DIR}")
        set(NATIVE_SOURCE_DIR "${CMAKE_SOURCE_DIR}")
        set(NATIVE_FLUTTER_ROOT "${FLUTTER_ROOT}")
        set(NATIVE_CMAKE_CMD "${CMAKE_COMMAND}")
        set(NATIVE_TOOL_TEMP_DIR "${CARGOKIT_TEMP_DIR}/tool")
    endif()

    set(CARGOKIT_ENV
        "CARGOKIT_CMAKE=${NATIVE_CMAKE_CMD}"
        "CARGOKIT_CONFIGURATION=$<CONFIG>"
        "CARGOKIT_MANIFEST_DIR=${NATIVE_MANIFEST_DIR}"
        "CARGOKIT_TARGET_TEMP_DIR=${NATIVE_TEMP_DIR}"
        "CARGOKIT_OUTPUT_DIR=${NATIVE_OUTPUT_DIR}"
        "CARGOKIT_TARGET_PLATFORM=${CARGOKIT_TARGET_PLATFORM}"
        "CARGOKIT_TOOL_TEMP_DIR=${NATIVE_TOOL_TEMP_DIR}"
        "CARGOKIT_ROOT_PROJECT_DIR=${NATIVE_SOURCE_DIR}"
        "FLUTTER_ROOT=${NATIVE_FLUTTER_ROOT}"
    )

    if (WIN32)
        set(SCRIPT_EXTENSION ".cmd")
        set(IMPORT_LIB_EXTENSION ".lib")
        # Convert script path to native format for Windows
        file(TO_NATIVE_PATH "${cargokit_cmake_root}/run_build_tool${SCRIPT_EXTENSION}" RUN_BUILD_TOOL_SCRIPT)
    else()
        set(SCRIPT_EXTENSION ".sh")
        set(IMPORT_LIB_EXTENSION "")
        execute_process(COMMAND chmod +x "${cargokit_cmake_root}/run_build_tool${SCRIPT_EXTENSION}")
        set(RUN_BUILD_TOOL_SCRIPT "${cargokit_cmake_root}/run_build_tool${SCRIPT_EXTENSION}")
    endif()

    # Using generators in custom command is only supported in CMake 3.20+
    if (CMAKE_CONFIGURATION_TYPES AND ${CMAKE_VERSION} VERSION_LESS "3.20.0")
        foreach(CONFIG IN LISTS CMAKE_CONFIGURATION_TYPES)
            add_custom_command(
                OUTPUT
                "${CMAKE_CURRENT_BINARY_DIR}/${CONFIG}/${CARGOKIT_LIB_FULL_NAME}"
                "${CMAKE_CURRENT_BINARY_DIR}/_phony_"
                COMMAND ${CMAKE_COMMAND} -E env ${CARGOKIT_ENV}
                    "${RUN_BUILD_TOOL_SCRIPT}"
                    build-cmake
                VERBATIM
            )
        endforeach()
    else()
        add_custom_command(
            OUTPUT
            ${OUTPUT_LIB}
            "${CMAKE_CURRENT_BINARY_DIR}/_phony_"
            COMMAND ${CMAKE_COMMAND} -E env ${CARGOKIT_ENV}
                "${RUN_BUILD_TOOL_SCRIPT}"
                build-cmake
            VERBATIM
        )
    endif()


    set_source_files_properties("${CMAKE_CURRENT_BINARY_DIR}/_phony_" PROPERTIES SYMBOLIC TRUE)

    if (TARGET ${target})
        # If we have actual cmake target provided create target and make existing
        # target depend on it
        add_custom_target("${target}_cargokit" DEPENDS ${OUTPUT_LIB})
        add_dependencies("${target}" "${target}_cargokit")
        target_link_libraries("${target}" PRIVATE "${OUTPUT_LIB}${IMPORT_LIB_EXTENSION}")
        if(WIN32)
            target_link_options(${target} PRIVATE "/INCLUDE:${any_symbol_name}")
        endif()
    else()
        # Otherwise (FFI) just use ALL to force building always
        add_custom_target("${target}_cargokit" ALL DEPENDS ${OUTPUT_LIB})
    endif()

    # Allow adding the output library to plugin bundled libraries
    set("${target}_cargokit_lib" ${OUTPUT_LIB} PARENT_SCOPE)

endfunction()
