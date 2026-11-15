# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The tests of tur-pax-companion that need no compositor. tst_pax_companion
# runs QML tests: every tst_*.qml in this directory by default, or whatever
# -input names (the reference pictures in shots/). The programs in unit/ are
# Qt Test: tst_pax_companion_logic needs nothing; tst_pax_companion_bus starts
# a private session bus with the stand-in for org.tur.Agent1 that P@x's
# window's tests use (apps/tur-pax-ui/tests: its answers are the real
# service's, recorded in CI), stand-ins for P@x's window and P@x's
# indicator, and the program itself. The tests that need KWin are in
# desktop/tests/compositor.

set(companion_fixtures "${CMAKE_CURRENT_SOURCE_DIR}/../fixtures")
set(paxui_tests "${CMAKE_SOURCE_DIR}/apps/tur-pax-ui/tests")

qt_add_executable(tst_pax_companion tst_main.cpp)
target_compile_definitions(tst_pax_companion PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}"
    TUR_COMPANION_FIXTURES="${companion_fixtures}")
target_link_libraries(tst_pax_companion PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest Qt6::DBus)
tur_link_qml_modules(tst_pax_companion turkit turkitpax turkittesting turpaxui turpaxcompanion)
add_test(NAME pax-companion-qml COMMAND tst_pax_companion)
set_tests_properties(pax-companion-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 900)

# The rules, the person's choices, the healing service's notices and the
# arithmetic of where things go.
qt_add_executable(tst_pax_companion_logic unit/tst_logic.cpp)
target_compile_definitions(tst_pax_companion_logic PRIVATE TUR_COMPANION_FIXTURES="${companion_fixtures}")
target_link_libraries(tst_pax_companion_logic PRIVATE Qt6::Core Qt6::Gui Qt6::DBus Qt6::Test turpaxcompanion)
add_test(NAME pax-companion-unit-logic COMMAND tst_pax_companion_logic)
set_tests_properties(pax-companion-unit-logic PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)

# The character against org.tur.Agent1 as the real service answered, and
# org.tur.PaxCompanion1 as the program offers it.
qt_add_executable(tst_pax_companion_bus unit/tst_bus.cpp ${paxui_tests}/fixturebus.h ${paxui_tests}/fixturebus.cpp)
target_include_directories(tst_pax_companion_bus PRIVATE ${paxui_tests})
target_compile_definitions(tst_pax_companion_bus PRIVATE
    TUR_PAX_UI_AGENT="$<TARGET_FILE:tur-pax-ui-fixture-agent>"
    TUR_PAX_UI_FIXTURES="${CMAKE_SOURCE_DIR}/apps/tur-pax-ui/fixtures/agent1"
    TUR_COMPANION_PROGRAM="$<TARGET_FILE:tur-pax-companion>"
    TUR_COMPANION_XML="${CMAKE_CURRENT_SOURCE_DIR}/../dbus/org.tur.PaxCompanion1.xml"
    TUR_COMPANION_FIXTURES="${companion_fixtures}")
target_link_libraries(tst_pax_companion_bus PRIVATE Qt6::Core Qt6::Gui Qt6::DBus Qt6::Test turpaxcompanion)
add_dependencies(tst_pax_companion_bus tur-pax-ui-fixture-agent tur-pax-companion)
add_test(NAME pax-companion-bus COMMAND tst_pax_companion_bus)
set_tests_properties(pax-companion-bus PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 600)
