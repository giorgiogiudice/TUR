# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-pax-ui: the window where a person talks to P@x (docs/architecture/
# tur-pax-ui.md; desktop-experience.md, sections 9.1, 9.2 and 9.12; task
# D-17). Its views and the C++ that talks to org.tur.Agent1 are one QML
# module, "Tur.PaxUi", which links nothing of KDE's, so that the tests can
# load a view without the window; tur-pax-ui is the window around them, and
# adds what only a Wayland session has: the layer-shell surface.

# Qt D-Bus, for org.tur.Agent1 and org.tur.PaxUi1. LGPL-3.0-only; allowed in
# desktop/ALLOWED_IMPORTS.
find_package(Qt6 6.8 REQUIRED COMPONENTS DBus)

set(paxui_qml
    qml/Main.qml
    qml/Words.qml
    qml/AskBar.qml
    qml/ConversationPanel.qml
    qml/ConversationEntry.qml
    qml/StepLine.qml
    qml/SystemCard.qml
    qml/KeysLine.qml
    qml/PaxStateNotice.qml
    qml/ActivityView.qml
    qml/ChoiceCard.qml
    qml/SetupFlow.qml
    qml/SetupScreen.qml
    qml/SetupLocal.qml
    qml/SetupProvider.qml
    qml/ProviderCard.qml
    qml/SetupKey.qml
    qml/SetupSettings.qml
    qml/SetupConsent.qml
)

# The sources are in qml/; the module is flat, so that every type sees every
# other without an import.
foreach(file IN LISTS paxui_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Words.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++ (desktop/README.md).
qt_add_qml_module(turpaxui
    URI Tur.PaxUi
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        src/agentclient.h
        src/agentclient.cpp
        src/conversation.h
        src/conversation.cpp
        src/uiservice.h
        src/uiservice.cpp
    QML_FILES ${paxui_qml}
)
target_include_directories(turpaxui PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_libraries(turpaxui PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus)
add_dependencies(turpaxui turkit turkitpax)

# The program's one library of KDE's, LGPL (read file by file at Plasma
# 6.8.2; tur-pax-overlay.md, section 8): LayerShellQt, for the surface.
find_package(LayerShellQt 6.0 CONFIG REQUIRED)

qt_add_executable(tur-pax-ui
    src/main.cpp
    src/surface.h
    src/surface.cpp
)
target_compile_definitions(tur-pax-ui PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-pax-ui PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus LayerShellQt::Interface)
tur_link_qml_modules(tur-pax-ui turkit turkitpax turpaxui)

# The program loads each view and reports no QML warning, under the offscreen
# platform, with no session bus: P@x's service is absent, which it says.
foreach(shown ask panel activity setup)
    add_test(NAME pax-ui-starts-${shown}
        COMMAND tur-pax-ui --standalone --check --no-dbus --mode ${shown})
    set_tests_properties(pax-ui-starts-${shown} PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
endforeach()
add_test(NAME pax-ui-starts-light
    COMMAND tur-pax-ui --standalone --check --no-dbus --mode panel --variant light)
set_tests_properties(pax-ui-starts-light PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)

# Asked to stop P@x where there is no session bus, it says so and fails;
# it needs no display.
add_test(NAME pax-ui-stop-without-a-bus COMMAND tur-pax-ui --stop)
set_tests_properties(pax-ui-stop-without-a-bus PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=none-such-platform;LC_ALL=C.UTF-8;ASAN_OPTIONS=detect_leaks=0;DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus"
    WILL_FAIL TRUE
    TIMEOUT 60)

# The desktop file is valid.
add_test(NAME pax-ui-desktop-file
    COMMAND desktop-file-validate ${CMAKE_CURRENT_SOURCE_DIR}/data/org.tur.PaxUi.desktop)
set_tests_properties(pax-ui-desktop-file PROPERTIES TIMEOUT 60)

add_subdirectory(tests)
