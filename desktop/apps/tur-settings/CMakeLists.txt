# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-settings: TUR's settings (docs/architecture/tur-settings.md). Its views
# and the C++ that reads the registries and answers on the session bus are
# one QML module, "Tur.Settings", so that the tests can load a view without
# the window; tur-settings is the window around them.

# Qt D-Bus, for org.tur.Settings1 and org.tur.Addressable1. LGPL-3.0-only;
# allowed in desktop/ALLOWED_IMPORTS.
find_package(Qt6 6.8 REQUIRED COMPONENTS DBus)

set(settings_qml
    qml/Main.qml
    qml/Shell.qml
    qml/Words.qml
    qml/SettingsPage.qml
    qml/SettingsGroup.qml
    qml/SettingRow.qml
    qml/RowHighlight.qml
    qml/LinkRow.qml
    qml/ChoiceOption.qml
    qml/DocumentText.qml
    qml/FailureMessage.qml
    qml/SearchResults.qml
    qml/OverviewPage.qml
    qml/AppearancePage.qml
    qml/AboutPage.qml
)

# The sources are in qml/; the module is flat, so that every type sees every
# other without an import.
foreach(file IN LISTS settings_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Words.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++ (desktop/README.md).
qt_add_qml_module(tursettings
    URI Tur.Settings
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        src/address.h
        src/address.cpp
        src/codes.h
        src/codes.cpp
        src/registrydata.h
        src/registrydata.cpp
        src/registry.h
        src/registry.cpp
        src/systemfacts.h
        src/systemfacts.cpp
        src/documents.h
        src/documents.cpp
        src/bustypes.h
        src/busadaptors.h
        src/busadaptors.cpp
        src/settingshost.h
        src/settingshost.cpp
    QML_FILES ${settings_qml}
)
target_include_directories(tursettings PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_libraries(tursettings PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::DBus)
add_dependencies(tursettings turkit turkitpax)

# What the program carries with it: its own settings registry and the one of
# the pages of KDE System Settings it links to (registry/), and the texts the
# About page shows word for word, which are the files at the top of the
# repository.
set(settings_registries
    registry/org.tur.Settings.json
    registry/org.kde.systemsettings.json
)
qt_add_resources(tursettings "tursettings_registry"
    PREFIX "/tur/settings"
    FILES ${settings_registries}
)
set(settings_texts
    ${CMAKE_SOURCE_DIR}/../LICENSE.md
    ${CMAKE_SOURCE_DIR}/../NOTICE
    ${CMAKE_SOURCE_DIR}/../THIRD_PARTY_NOTICES.md
)
foreach(file IN LISTS settings_texts)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
qt_add_resources(tursettings "tursettings_texts"
    PREFIX "/tur/settings/texts"
    FILES ${settings_texts}
)

qt_add_executable(tur-settings src/main.cpp)
target_compile_definitions(tur-settings PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-settings PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::DBus)
tur_link_qml_modules(tur-settings turkit turkitpax tursettings)

# Into the image: the program, its module's library, its launcher entry, its
# bus activation file, its entry in KDE System Settings' side list
# (configuration, not a module: desktop-experience.md, 10.1) and its
# interface descriptions.
tur_install_program(tur-settings)
tur_install_modules(tursettings)
tur_install_data(applications data/org.tur.Settings.desktop)
tur_install_data(dbus-1/services data/org.tur.Settings1.service)
tur_install_data(plasma/systemsettings/externalmodules data/systemsettings/org.tur.Settings.desktop)
tur_install_data(dbus-1/interfaces dbus/org.tur.Settings1.xml dbus/org.tur.Addressable1.xml)

# The program starts on each page, on a row and on a link, and reports no QML
# warning. An address it cannot open is refused, and it says so.
set(settings_fixtures ${CMAKE_CURRENT_SOURCE_DIR}/fixtures)
set(settings_start tur-settings --standalone --check --fixture ${settings_fixtures}/default)
add_test(NAME settings-starts COMMAND ${settings_start})
add_test(NAME settings-starts-on-a-page COMMAND ${settings_start} tur.appearance)
add_test(NAME settings-starts-on-a-row COMMAND ${settings_start} "tur.about#source-revision")
add_test(NAME settings-starts-on-a-link COMMAND ${settings_start} --developer "tur-settings:tur.appearance#variant")
add_test(NAME settings-starts-where-there-is-no-tur
    COMMAND tur-settings --standalone --check --fixture ${settings_fixtures}/not-tur tur.about)
add_test(NAME settings-refuses-an-unknown-address COMMAND ${settings_start} "tur.nothing#at-all")
add_test(NAME settings-refuses-a-page-that-is-not-built COMMAND ${settings_start} "tur.pax.privacy#private-mode")
set_tests_properties(settings-refuses-an-unknown-address PROPERTIES
    PASS_REGULAR_EXPRESSION "TUR-DESKTOP-SETTINGS-001: tur.nothing#at-all")
set_tests_properties(settings-refuses-a-page-that-is-not-built PROPERTIES
    PASS_REGULAR_EXPRESSION "TUR-DESKTOP-SETTINGS-003: tur.pax.privacy#private-mode")

# Asked only a question, it answers from the registries and opens no window:
# it needs no display, which is how the assistant's sandbox will run it.
add_test(NAME settings-answers-a-search COMMAND tur-settings --search "dark mode")
set_tests_properties(settings-answers-a-search PROPERTIES
    PASS_REGULAR_EXPRESSION "\"address\": \"tur.appearance#variant\"")
add_test(NAME settings-answers-a-resolve COMMAND tur-settings --resolve "tur.about#version")
set_tests_properties(settings-answers-a-resolve PROPERTIES PASS_REGULAR_EXPRESSION "\"program\": \"org.tur.Settings\"")
add_test(NAME settings-answers-a-list COMMAND tur-settings --list)
set_tests_properties(settings-answers-a-list PROPERTIES PASS_REGULAR_EXPRESSION "\"page\": \"tur.overview\"")

set_tests_properties(
    settings-starts
    settings-starts-on-a-page
    settings-starts-on-a-row
    settings-starts-on-a-link
    settings-starts-where-there-is-no-tur
    settings-refuses-an-unknown-address
    settings-refuses-a-page-that-is-not-built
    PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
# No display at all for the three questions.
set_tests_properties(
    settings-answers-a-search
    settings-answers-a-resolve
    settings-answers-a-list
    PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=none-such-platform;LC_ALL=C.UTF-8;ASAN_OPTIONS=detect_leaks=0" TIMEOUT 60)

# The desktop files are valid.
add_test(NAME settings-desktop-files
    COMMAND desktop-file-validate
        ${CMAKE_CURRENT_SOURCE_DIR}/data/org.tur.Settings.desktop
        ${CMAKE_CURRENT_SOURCE_DIR}/data/systemsettings/org.tur.Settings.desktop)
set_tests_properties(settings-desktop-files PROPERTIES TIMEOUT 60)

add_subdirectory(tests)
