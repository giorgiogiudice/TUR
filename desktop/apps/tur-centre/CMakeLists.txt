# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-centre: the application centre (docs/architecture/tur-centre.md). Its
# views and the small C++ layer that asks the catalogue are one QML module,
# "Tur.Centre", so that the tests can load a view without the window;
# tur-centre is the window around them.

set(centre_qml
    qml/Main.qml
    qml/Shell.qml
    qml/Words.qml
    qml/HomeView.qml
    qml/SearchView.qml
    qml/ResultRow.qml
    qml/ApplicationView.qml
    qml/ApplicationRoutes.qml
    qml/NoRoute.qml
    qml/WayBlock.qml
    qml/RatingLine.qml
    qml/RatingGlyph.qml
    qml/CaveatRow.qml
    qml/Facts.qml
    qml/LinkButton.qml
    qml/BackButton.qml
    qml/AppTile.qml
    qml/FailureMessage.qml
    qml/RouterWords.qml
    qml/ChoiceRow.qml
    qml/InstallSheet.qml
    qml/RemoveSheet.qml
    qml/InstallerDialog.qml
    qml/OperationBlock.qml
    qml/OnThisComputer.qml
    qml/InstalledRow.qml
    qml/InstalledView.qml
    qml/UpdatesView.qml
    qml/SourcesView.qml
)

# The sources are in qml/; the module is flat, so that every type sees every
# other without an import.
foreach(file IN LISTS centre_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Words.qml qml/RouterWords.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++ (desktop/README.md).
qt_add_qml_module(turcentre
    URI Tur.Centre
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        src/cataloguesource.h
        src/cataloguesource.cpp
        src/processsource.h
        src/processsource.cpp
        src/fixturesource.h
        src/fixturesource.cpp
        src/answers.h
        src/answers.cpp
        src/hitsmodel.h
        src/hitsmodel.cpp
        src/catalogue.h
        src/catalogue.cpp
        src/appssource.h
        src/appssource.cpp
        src/appsprocess.h
        src/appsprocess.cpp
        src/appsfixture.h
        src/appsfixture.cpp
        src/apps.h
        src/apps.cpp
    QML_FILES ${centre_qml}
)
target_include_directories(turcentre PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_libraries(turcentre PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick)
add_dependencies(turcentre turkit)

qt_add_executable(tur-centre src/main.cpp)
target_compile_definitions(tur-centre PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-centre PRIVATE Qt6::Core Qt6::Gui Qt6::Qml)
tur_link_qml_modules(tur-centre turkit turcentre)

# The program starts on recorded answers, shows a search and an application's
# page, and reports no QML warning.
set(centre_fixtures ${CMAKE_CURRENT_SOURCE_DIR}/fixtures)
add_test(NAME centre-starts
    COMMAND tur-centre --standalone --check --fixture ${centre_fixtures}/default)
add_test(NAME centre-starts-on-a-search
    COMMAND tur-centre --standalone --check --fixture ${centre_fixtures}/default tur-centre:search?q=office)
add_test(NAME centre-starts-on-an-application
    COMMAND tur-centre --standalone --check --fixture ${centre_fixtures}/default tur-centre:app/com.microsoft.office)
add_test(NAME centre-starts-on-no-route
    COMMAND tur-centre --standalone --check --fixture ${centre_fixtures}/default tur-centre:app/com.riotgames.valorant)
# Without a catalogue program it still starts, and says what is wrong.
add_test(NAME centre-starts-without-the-catalogue
    COMMAND tur-centre --standalone --check --catalogue-program /nonexistent/tur-catalogue tur-centre:search?q=office)
set_tests_properties(
    centre-starts
    centre-starts-on-a-search
    centre-starts-on-an-application
    centre-starts-on-no-route
    centre-starts-without-the-catalogue
    PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)

add_subdirectory(tests)
