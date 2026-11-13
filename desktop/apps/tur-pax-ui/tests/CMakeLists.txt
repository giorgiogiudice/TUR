# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The tests of tur-pax-ui. tst_paxui runs QML tests: every tst_*.qml in this
# directory by default, or whatever -input names (the pictures in shots/,
# the end-to-end test in e2e/). They run against a stand-in for
# org.tur.Agent1 (fixture/agent.cpp) on a private session bus, answering
# from what the real service answered in CI (../fixtures/agent1). The
# programs in unit/ are Qt Test.

# The stand-in for P@x's service. Not shipped.
qt_add_executable(tur-pax-ui-fixture-agent fixture/agent.cpp)
target_link_libraries(tur-pax-ui-fixture-agent PRIVATE Qt6::Core Qt6::DBus)

set(paxui_test_definitions
    TUR_PAX_UI_AGENT="$<TARGET_FILE:tur-pax-ui-fixture-agent>"
    TUR_PAX_UI_FIXTURES="${CMAKE_CURRENT_SOURCE_DIR}/../fixtures/agent1"
    TUR_PAX_COMMANDS="${CMAKE_SOURCE_DIR}/kit/pax/commands.json"
)

qt_add_executable(tst_paxui tst_main.cpp fixturebus.h fixturebus.cpp)
target_compile_definitions(tst_paxui PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}"
    ${paxui_test_definitions})
target_link_libraries(tst_paxui PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest Qt6::DBus)
tur_link_qml_modules(tst_paxui turkit turkitpax turkittesting turpaxui)
add_dependencies(tst_paxui tur-pax-ui-fixture-agent)
add_test(NAME pax-ui-qml COMMAND tst_paxui)
set_tests_properties(pax-ui-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 1200)

# The conversation model, fed the recorded signals of the real service.
qt_add_executable(tst_paxui_conversation unit/tst_conversation.cpp)
target_compile_definitions(tst_paxui_conversation PRIVATE ${paxui_test_definitions})
target_link_libraries(tst_paxui_conversation PRIVATE Qt6::Core Qt6::Gui Qt6::Test turpaxui)
add_test(NAME pax-ui-unit-conversation COMMAND tst_paxui_conversation)
set_tests_properties(pax-ui-unit-conversation PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)

# The client of org.tur.Agent1 and the program's stop key, on a private bus
# with the stand-in; and the client's table of methods against the
# service's committed interface file.
qt_add_executable(tst_paxui_client unit/tst_client.cpp fixturebus.h fixturebus.cpp)
target_include_directories(tst_paxui_client PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
target_compile_definitions(tst_paxui_client PRIVATE
    ${paxui_test_definitions}
    TUR_PAX_UI_PROGRAM="$<TARGET_FILE:tur-pax-ui>"
    TUR_PAX_UI_SOURCE="${CMAKE_CURRENT_SOURCE_DIR}/.."
    TUR_AGENT1_INTERFACE="${CMAKE_SOURCE_DIR}/../services/tur-pax/interfaces/org.tur.Agent1.xml")
target_link_libraries(tst_paxui_client PRIVATE Qt6::Core Qt6::Gui Qt6::DBus Qt6::Test turpaxui)
add_dependencies(tst_paxui_client tur-pax-ui-fixture-agent tur-pax-ui)
add_test(NAME pax-ui-unit-client COMMAND tst_paxui_client)
set_tests_properties(pax-ui-unit-client PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)
