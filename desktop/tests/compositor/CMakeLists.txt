# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The tests that need a running compositor (desktop-experience.md, 15.3 and
# 15.4: the `compositor` job). tst_compositor is a Wayland client of a real
# KWin, started headless by desktop/ci/compositor.sh in the builder image; it
# starts tur-settings and tur-pax-overlay as Wayland clients beside it on a
# private session bus, drives them, injects input and takes screenshots from
# the compositor.
#
# It is not a ctest test: without KWin it cannot run. The `compositor` job
# runs it once for each arrangement of outputs, scale and variant.
#
# What it links beyond Qt: KWayland's client library (LGPL), for the Plasma
# window-management protocol (where KWin says the windows are, and which is
# active) and the fake-input protocol (the input the test injects). Neither is
# shipped in a TUR program for input: the overlay never injects anything.

qt_add_executable(tst_compositor tst_compositor.cpp probewindow.h probewindow.cpp)
target_compile_definitions(tst_compositor PRIVATE
    TUR_SETTINGS_PROGRAM="$<TARGET_FILE:tur-settings>"
    TUR_SETTINGS_FIXTURES="${CMAKE_SOURCE_DIR}/apps/tur-settings/fixtures"
    TUR_SETTINGS_HELPER="$<TARGET_FILE:tst_settings_helper>"
    TUR_PAX_OVERLAY_PROGRAM="$<TARGET_FILE:tur-pax-overlay>"
    TUR_PAX_OVERLAY_XML="${CMAKE_SOURCE_DIR}/apps/tur-pax-overlay/dbus/org.tur.PaxOverlay1.xml")
target_link_libraries(tst_compositor PRIVATE
    Qt6::Core Qt6::Gui Qt6::DBus Qt6::Test Plasma::KWaylandClient turpaxoverlay)
add_dependencies(tst_compositor tur-settings tur-pax-overlay tst_settings_helper)
