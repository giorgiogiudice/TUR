# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The tests of tur-pax-overlay that need no compositor. tst_pax_overlay runs
# QML tests: every tst_*.qml in this directory by default, or whatever -input
# names (the reference pictures in shots/). The programs in unit/ are Qt
# Test: tst_pax_overlay_logic needs nothing; tst_pax_overlay_bus starts a
# private session bus and the program itself. The tests that need KWin are
# in desktop/tests/compositor.

qt_add_executable(tst_pax_overlay tst_main.cpp)
target_compile_definitions(tst_pax_overlay PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}"
    TUR_PAX_COMMANDS="${CMAKE_SOURCE_DIR}/kit/pax/commands.json")
target_link_libraries(tst_pax_overlay PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest)
tur_link_qml_modules(tst_pax_overlay turkit turkitpax turkittesting turpaxoverlay)
add_test(NAME pax-overlay-qml COMMAND tst_pax_overlay)
set_tests_properties(pax-overlay-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 600)

qt_add_executable(tst_pax_overlay_logic unit/tst_logic.cpp)
target_link_libraries(tst_pax_overlay_logic PRIVATE Qt6::Core Qt6::Gui Qt6::DBus Qt6::Test turpaxoverlay)
add_test(NAME pax-overlay-unit-logic COMMAND tst_pax_overlay_logic)
set_tests_properties(pax-overlay-unit-logic PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)

# org.tur.PaxOverlay1 on a private session bus (dbus-daemon, started by the
# test), offered by the program itself under the offscreen platform.
qt_add_executable(tst_pax_overlay_bus unit/tst_bus.cpp)
target_compile_definitions(tst_pax_overlay_bus PRIVATE
    TUR_PAX_OVERLAY_PROGRAM="$<TARGET_FILE:tur-pax-overlay>"
    TUR_PAX_OVERLAY_SOURCE="${CMAKE_CURRENT_SOURCE_DIR}/..")
target_link_libraries(tst_pax_overlay_bus PRIVATE Qt6::Core Qt6::DBus Qt6::Test turpaxoverlay)
add_dependencies(tst_pax_overlay_bus tur-pax-overlay)
add_test(NAME pax-overlay-bus COMMAND tst_pax_overlay_bus)
set_tests_properties(pax-overlay-bus PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)
