# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Tur.Kit.Testing: the test harness every TUR program's QML tests use: the
# direction and text-length harness, the contrast walk and the accessibility
# walk. Linked by test programs only; not installed in the image.

qt_add_qml_module(turkittesting
    URI Tur.Kit.Testing
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        harness.h
        harness.cpp
        pseudotranslator.h
        pseudotranslator.cpp
)
target_compile_definitions(turkittesting PRIVATE
    TUR_TOKENS_JSON="${CMAKE_CURRENT_SOURCE_DIR}/../tokens/tokens.json")
target_include_directories(turkittesting PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(turkittesting PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick PRIVATE Qt6::QuickControls2)
