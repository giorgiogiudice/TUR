# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-pax-companion: P@x's character on the bar (docs/architecture/
# tur-pax-companion.md). Its drawing and its rules are one QML module,
# "Tur.PaxCompanion", which links nothing of KDE's, so that the tests can load
# the character without a compositor; tur-pax-companion is the program
# around them, and adds what only a Wayland session has: the layer-shell
# surface on the bar.
#
# It is a standalone program under the project's licence. Nothing of it runs
# inside Plasma: no applet, no KWin script (design, section 2).

# Qt D-Bus, for org.tur.Agent1, org.tur.PaxCompanion1 and the three
# properties the character follows. LGPL-3.0-only; allowed in
# desktop/ALLOWED_IMPORTS.
find_package(Qt6 6.8 REQUIRED COMPONENTS DBus)

set(companion_qml
    qml/Main.qml
    qml/Words.qml
    qml/Stage.qml
    qml/Character.qml
    qml/StateMark.qml
    qml/Leaf.qml
    qml/Bubble.qml
    qml/NoticeCard.qml
    qml/MenuCard.qml
    qml/WatchingCard.qml
    qml/SettingsCard.qml
)

# The sources are in qml/; the module is flat, so that every type sees every
# other without an import.
foreach(file IN LISTS companion_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Words.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++ (desktop/README.md).
qt_add_qml_module(turpaxcompanion
    URI Tur.PaxCompanion
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        src/companionsettings.h
        src/companionsettings.cpp
        src/companion.h
        src/companion.cpp
        src/placement.h
        src/placement.cpp
        src/busproperty.h
        src/busproperty.cpp
        src/agentsource.h
        src/agentsource.cpp
        src/healsource.h
        src/healsource.cpp
        src/companionservice.h
        src/companionservice.cpp
    QML_FILES ${companion_qml}
)
target_include_directories(turpaxcompanion PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src)
# libturpaxui: the client of org.tur.Agent1 (AgentClient), which is P@x's
# window's and is used here as it is, so that the character and the window
# cannot understand the service differently.
target_link_libraries(turpaxcompanion PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus turpaxui)
add_dependencies(turpaxcompanion turkit turkitpax)

# The program's one library of KDE's, LGPL (read file by file at Plasma
# 6.8.2; tur-pax-overlay.md, section 8): LayerShellQt, for the surfaces.
find_package(LayerShellQt 6.0 CONFIG REQUIRED)

qt_add_executable(tur-pax-companion
    src/main.cpp
    src/surfaces.h
    src/surfaces.cpp
)
target_compile_definitions(tur-pax-companion PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-pax-companion PRIVATE
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus LayerShellQt::Interface)
tur_link_qml_modules(tur-pax-companion turkit turkitpax turpaxui turpaxcompanion)

# The program loads the character and reports no QML warning, under the
# offscreen platform, with no session bus: P@x is absent, so the character is
# asleep. The settings file only says that the person keeps it on the bar
# asleep, so that the strip is really drawn.
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/starts-asleep.json "{\"version\": 1, \"presence\": \"always\"}\n")
foreach(look dark light)
    add_test(NAME pax-companion-starts-${look}
        COMMAND tur-pax-companion --standalone --check --no-dbus --variant ${look}
                --settings ${CMAKE_CURRENT_BINARY_DIR}/starts-asleep.json)
    set_tests_properties(pax-companion-starts-${look} PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
endforeach()

# Asked to hide where there is no session bus, it writes the choice down for
# the next character and needs no display.
add_test(NAME pax-companion-hide-without-a-bus
    COMMAND tur-pax-companion --hide --settings ${CMAKE_CURRENT_BINARY_DIR}/hide.json)
set_tests_properties(pax-companion-hide-without-a-bus PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=none-such-platform;LC_ALL=C.UTF-8;ASAN_OPTIONS=detect_leaks=0;DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus"
    TIMEOUT 60)

# The desktop file is valid.
add_test(NAME pax-companion-desktop-file
    COMMAND desktop-file-validate ${CMAKE_CURRENT_SOURCE_DIR}/data/org.tur.PaxCompanion.desktop)
set_tests_properties(pax-companion-desktop-file PROPERTIES TIMEOUT 60)

add_subdirectory(tests)
