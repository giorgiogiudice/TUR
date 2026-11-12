# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The installer's tests. tst_installer runs QML tests: every tst_*.qml in
# this directory by default, or whatever -input names (the pictures in
# shots/). tst_installer_keyboard is Qt Test.

qt_add_executable(tst_installer tst_main.cpp)
target_compile_definitions(tst_installer PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}"
    TUR_INSTALLER_FIXTURES="${CMAKE_CURRENT_SOURCE_DIR}/../fixtures"
    TUR_INSTALLER_SCRATCH="${CMAKE_CURRENT_BINARY_DIR}/scratch")
target_link_libraries(tst_installer PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest)
tur_link_qml_modules(tst_installer turkit turkittesting turinstaller)

add_test(NAME installer-qml COMMAND tst_installer)
set_tests_properties(installer-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 900)

# Every screen, dark and light, as pictures, into the desktop workflow's
# artifact (out/desktop/tur-installer-shots). They are not compared with
# references: desktop/ci/screenshots.sh, which compares, is outside this
# program's directory. A person looks at them (docs/reports/TUR-009.md).
add_test(NAME installer-pictures
    COMMAND tst_installer -input ${CMAKE_CURRENT_SOURCE_DIR}/shots/render_installer.qml)
set_tests_properties(installer-pictures PROPERTIES
    ENVIRONMENT "${TUR_TEST_ENVIRONMENT};TUR_SCREENSHOT_DIR=${CMAKE_SOURCE_DIR}/../out/desktop/tur-installer-shots/${CMAKE_BUILD_TYPE}"
    TIMEOUT 600)

qt_add_executable(tst_installer_keyboard unit/tst_keyboard.cpp)
target_compile_definitions(tst_installer_keyboard PRIVATE
    TUR_INSTALLER_SCRATCH="${CMAKE_CURRENT_BINARY_DIR}/scratch-keyboard")
target_link_libraries(tst_installer_keyboard PRIVATE Qt6::Core Qt6::Gui Qt6::Test turinstaller)
add_test(NAME installer-unit-keyboard COMMAND tst_installer_keyboard)
set_tests_properties(installer-unit-keyboard PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
