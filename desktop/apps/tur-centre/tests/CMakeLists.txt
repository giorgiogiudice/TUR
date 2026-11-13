# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The application centre's tests. tst_centre runs QML tests: every tst_*.qml
# in this directory by default, or whatever -input names (the screenshots in
# shots/). The programs in unit/ are Qt Test.

set(centre_test_definitions
    TUR_CENTRE_FIXTURES="${CMAKE_CURRENT_SOURCE_DIR}/../fixtures"
)

# A stand-in for tur-catalogue that misbehaves on request: it sleeps, floods,
# crashes or prints nonsense, so that the tests can show what ProcessSource
# does about each. It is not Qt code and is not shipped.
add_executable(tst_centre_helper unit/helper.cpp)

qt_add_executable(tst_centre tst_main.cpp)
target_compile_definitions(tst_centre PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}"
    TUR_CENTRE_HELPER="$<TARGET_FILE:tst_centre_helper>"
    ${centre_test_definitions})
target_link_libraries(tst_centre PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest)
tur_link_qml_modules(tst_centre turkit turkittesting turcentre)
add_dependencies(tst_centre tst_centre_helper)

add_test(NAME centre-qml COMMAND tst_centre)
set_tests_properties(centre-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 900)

# How the centre runs tur-apps, against a stand-in that misbehaves on request
# (unit/apps-helper.sh) and the recorded runs of the real one.
qt_add_executable(tst_centre_apps unit/tst_apps.cpp)
target_compile_definitions(tst_centre_apps PRIVATE
    ${centre_test_definitions}
    TUR_CENTRE_APPS_HELPER="${CMAKE_CURRENT_SOURCE_DIR}/unit/apps-helper.sh")
target_link_libraries(tst_centre_apps PRIVATE Qt6::Core Qt6::Gui Qt6::Test turcentre)
add_test(NAME centre-unit-apps COMMAND tst_centre_apps)
set_tests_properties(centre-unit-apps PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)

foreach(name answers sources catalogue)
    qt_add_executable(tst_centre_${name} unit/tst_${name}.cpp)
    target_compile_definitions(tst_centre_${name} PRIVATE
        ${centre_test_definitions}
        TUR_CENTRE_HELPER="$<TARGET_FILE:tst_centre_helper>")
    target_link_libraries(tst_centre_${name} PRIVATE Qt6::Core Qt6::Gui Qt6::Test turcentre)
    add_dependencies(tst_centre_${name} tst_centre_helper)
    add_test(NAME centre-unit-${name} COMMAND tst_centre_${name})
    set_tests_properties(centre-unit-${name} PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)
endforeach()
