# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-pax-overlay: P@x's indicator, pointer and highlights
# (docs/architecture/tur-pax-overlay.md). The drawing and the logic of the
# leases are one QML module and library, "Tur.PaxOverlay", which links
# nothing of KDE's, so that the tests can load them under the offscreen
# platform. The program adds what only a Wayland session has: layer-shell
# surfaces (LayerShellQt) and the window locator (KWayland).

# Qt D-Bus, for org.tur.PaxOverlay1. LGPL-3.0-only; allowed in
# desktop/ALLOWED_IMPORTS.
find_package(Qt6 6.8 REQUIRED COMPONENTS DBus)

set(overlay_qml
    qml/OverlaySurface.qml
    qml/ActivityPill.qml
    qml/CommandsCard.qml
    qml/StateGlyph.qml
    qml/PaxPointer.qml
    qml/PointerRing.qml
    qml/HighlightMark.qml
    qml/DimLayer.qml
    qml/CaptionBubble.qml
    qml/ScreenFrame.qml
    qml/Words.qml
)
foreach(file IN LISTS overlay_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Words.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++ (desktop/README.md).
qt_add_qml_module(turpaxoverlay
    URI Tur.PaxOverlay
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        src/bustypes.h
        src/bustypes.cpp
        src/windowlocator.h
        src/windowlocator.cpp
        src/overlaystate.h
        src/overlaystate.cpp
        src/overlayservice.h
        src/overlayservice.cpp
        src/busadaptor.h
        src/busadaptor.cpp
    QML_FILES ${overlay_qml}
)
target_include_directories(turpaxoverlay PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_libraries(turpaxoverlay PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus)
add_dependencies(turpaxoverlay turkit turkitpax)

# The two libraries of the program, both LGPL (checked file by file at
# Plasma 6.8.2; tur-pax-overlay.md, section 8): LayerShellQt for the
# layer-shell surfaces, and KWayland's client library for the Plasma
# window-management protocol.
find_package(LayerShellQt 6.0 CONFIG REQUIRED)
find_package(KWayland 6.0 CONFIG REQUIRED)

qt_add_executable(tur-pax-overlay
    src/main.cpp
    src/surfaces.h
    src/surfaces.cpp
    src/kwinlocator.h
    src/kwinlocator.cpp
)
target_compile_definitions(tur-pax-overlay PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-pax-overlay PRIVATE
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus
    LayerShellQt::Interface Plasma::KWaylandClient)
tur_link_qml_modules(tur-pax-overlay turkit turkitpax turpaxoverlay)

# Into the image: the program, its module's library, its bus activation file
# (the overlay has no launcher entry: it is started when something calls
# org.tur.PaxOverlay1) and its interface description.
tur_install_program(tur-pax-overlay)
tur_install_modules(turpaxoverlay)
tur_install_data(dbus-1/services data/org.tur.PaxOverlay1.service)
tur_install_data(dbus-1/interfaces dbus/org.tur.PaxOverlay1.xml)

# The program loads its surfaces and reports no QML warning, under the
# offscreen platform (where it has plain windows and no window locator).
add_test(NAME pax-overlay-starts COMMAND tur-pax-overlay --check)
set_tests_properties(pax-overlay-starts PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
add_subdirectory(tests)

# The tests that need a running KWin (desktop-experience.md, 15.3: the
# `compositor` job). They live with the other cross-program tests under
# desktop/tests/compositor and are built here, beside the programs they run.
add_subdirectory(${CMAKE_SOURCE_DIR}/tests/compositor ${CMAKE_BINARY_DIR}/tests/compositor)
