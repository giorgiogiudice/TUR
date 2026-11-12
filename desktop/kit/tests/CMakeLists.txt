# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The kit's tests. tst_kit runs QML tests: every tst_*.qml in this directory
# by default, or whatever -input names (the screenshots in shots/, the
# toolchain proof in desktop/ci/probe). The programs in unit/ are Qt Test.

qt_add_executable(tst_kit tst_main.cpp)
target_compile_definitions(tst_kit PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}")
target_link_libraries(tst_kit PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest)
tur_link_qml_modules(tst_kit turkit turkitpax turkitgallery turkittesting)

add_test(NAME kit-qml COMMAND tst_kit)
set_tests_properties(kit-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 600)

qt_add_executable(tst_systemappearance unit/tst_systemappearance.cpp)
target_link_libraries(tst_systemappearance PRIVATE Qt6::Core Qt6::Gui Qt6::Test turkit)
add_test(NAME kit-unit-systemappearance COMMAND tst_systemappearance)
set_tests_properties(kit-unit-systemappearance PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)

qt_add_executable(tst_pseudotranslator unit/tst_pseudotranslator.cpp)
target_link_libraries(tst_pseudotranslator PRIVATE Qt6::Core Qt6::Gui Qt6::Test turkittesting)
add_test(NAME kit-unit-pseudotranslator COMMAND tst_pseudotranslator)
set_tests_properties(kit-unit-pseudotranslator PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
