# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-installer: TUR's graphical installer (desktop-experience.md, 11.5;
# task D-23), in the live session of the installer medium (ADR-0018,
# amendment of 13 November 2026). Its screens and the C++ that talks to the
# installation engine are one QML module, "Tur.Installer", so that the tests
# can load the wizard without the window; tur-installer-wizard is the window
# around them. The installer medium builds the program from this directory
# (installer/iso/Containerfile) against the image's own Qt.

# Qt D-Bus, to tell KWin that the keyboard layout changed. LGPL-3.0-only;
# allowed in desktop/ALLOWED_IMPORTS.
find_package(Qt6 6.8 REQUIRED COMPONENTS DBus)

set(installer_qml
    qml/Main.qml
    qml/Wizard.qml
    qml/Words.qml
    qml/WizardScreen.qml
    qml/WelcomeScreen.qml
    qml/ComputerScreen.qml
    qml/DiskScreen.qml
    qml/DiskCard.qml
    qml/PartitionBar.qml
    qml/AccountScreen.qml
    qml/ConfirmScreen.qml
    qml/ProgressScreen.qml
    qml/DoneScreen.qml
    qml/DiagnosticsDrawer.qml
    qml/LogSheet.qml
    qml/SaveLogSheet.qml
)

# The sources are in qml/; the module is flat, so that every type sees every
# other without an import.
foreach(file IN LISTS installer_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Words.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++ (desktop/README.md).
qt_add_qml_module(turinstaller
    URI Tur.Installer
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        src/engine.h
        src/engine.cpp
        src/keyboard.h
        src/keyboard.cpp
        src/transcript.h
        src/transcript.cpp
    QML_FILES ${installer_qml}
)
target_include_directories(turinstaller PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_libraries(turinstaller PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus)
add_dependencies(turinstaller turkit)

qt_add_executable(tur-installer-wizard src/main.cpp)
target_compile_definitions(tur-installer-wizard PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-installer-wizard PRIVATE Qt6::Core Qt6::Gui Qt6::Qml)
tur_link_qml_modules(tur-installer-wizard turkit turinstaller)

# The program starts on recorded answers, on each set of them, and reports no
# QML warning.
set(installer_fixtures ${CMAKE_CURRENT_SOURCE_DIR}/fixtures)
add_test(NAME installer-starts
    COMMAND tur-installer-wizard --standalone --check --fixture ${installer_fixtures}/beside-windows)
add_test(NAME installer-starts-light
    COMMAND tur-installer-wizard --standalone --check --variant light --fixture ${installer_fixtures}/beside-windows)
set_tests_properties(installer-starts installer-starts-light PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)

# The desktop files are valid.
add_test(NAME installer-desktop-files
    COMMAND desktop-file-validate
        ${CMAKE_SOURCE_DIR}/../installer/iso/rootfs/usr/share/applications/org.tur.Installer.desktop
        ${CMAKE_SOURCE_DIR}/../installer/iso/rootfs/etc/xdg/autostart/org.tur.Installer.desktop)
set_tests_properties(installer-desktop-files PROPERTIES TIMEOUT 60)

add_subdirectory(tests)
