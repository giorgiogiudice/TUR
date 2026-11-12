# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The tests of tur-settings. tst_settings runs QML tests: every tst_*.qml in
# this directory by default, or whatever -input names (the screenshots in
# shots/). The programs in unit/ are Qt Test; tst_settings_bus is the one
# that needs a session bus, and starts a private one.

set(settings_test_definitions
    TUR_SETTINGS_FIXTURES="${CMAKE_CURRENT_SOURCE_DIR}/../fixtures"
    TUR_SETTINGS_SOURCE="${CMAKE_CURRENT_SOURCE_DIR}/.."
    TUR_SETTINGS_TEST_DATA="${CMAKE_CURRENT_SOURCE_DIR}/data"
    TUR_VERSION="${PROJECT_VERSION}"
)

# A stand-in for KDE System Settings: it writes the arguments it was given to
# the file TUR_SETTINGS_HELPER_OUT names, so that a test can see that the
# right module was asked for. It is not Qt code and is not shipped.
add_executable(tst_settings_helper unit/helper.cpp)

# A stand-in for another TUR program with addressable content: it owns
# org.tur.Example on the session bus and answers org.tur.Addressable1. Not
# shipped.
qt_add_executable(tst_settings_peer unit/peer.cpp)
target_link_libraries(tst_settings_peer PRIVATE Qt6::Core Qt6::DBus)

qt_add_executable(tst_settings tst_main.cpp)
target_compile_definitions(tst_settings PRIVATE
    QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
    TUR_QML_IMPORT_DIR="${QT_QML_OUTPUT_DIRECTORY}"
    TUR_SETTINGS_HELPER="$<TARGET_FILE:tst_settings_helper>"
    ${settings_test_definitions})
target_link_libraries(tst_settings PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::QuickTest)
tur_link_qml_modules(tst_settings turkit turkitpax turkittesting tursettings)
add_dependencies(tst_settings tst_settings_helper)

add_test(NAME settings-qml COMMAND tst_settings)
set_tests_properties(settings-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 900)

foreach(name registry facts)
    qt_add_executable(tst_settings_${name} unit/tst_${name}.cpp)
    target_compile_definitions(tst_settings_${name} PRIVATE ${settings_test_definitions})
    target_link_libraries(tst_settings_${name} PRIVATE Qt6::Core Qt6::Gui Qt6::Test tursettings)
    add_test(NAME settings-unit-${name} COMMAND tst_settings_${name})
    set_tests_properties(settings-unit-${name} PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 300)
endforeach()

# The addressing, over D-Bus, on a private session bus (dbus-daemon, started
# by the test): the window in the test's own process, then the program
# itself, started by hand and started by the bus.
qt_add_executable(tst_settings_bus unit/tst_bus.cpp)
target_compile_definitions(tst_settings_bus PRIVATE
    ${settings_test_definitions}
    TUR_SETTINGS_PROGRAM="$<TARGET_FILE:tur-settings>"
    TUR_SETTINGS_HELPER="$<TARGET_FILE:tst_settings_helper>"
    TUR_SETTINGS_PEER="$<TARGET_FILE:tst_settings_peer>")
target_link_libraries(tst_settings_bus PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus Qt6::Test)
tur_link_qml_modules(tst_settings_bus turkit turkitpax tursettings)
add_dependencies(tst_settings_bus tur-settings tst_settings_helper tst_settings_peer)
add_test(NAME settings-bus COMMAND tst_settings_bus)
set_tests_properties(settings-bus PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 600)
