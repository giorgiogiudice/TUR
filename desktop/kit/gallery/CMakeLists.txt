# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The gallery: every component of the kit in every state. Its pages are a QML
# module, so that the tests can load them one by one; tur-kit-gallery is the
# window around them.

qt_add_qml_module(turkitgallery
    URI Tur.Kit.Gallery
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    QML_FILES
        Main.qml
        GalleryPage.qml
        Specimen.qml
        Swatch.qml
        FoundationsPage.qml
        ButtonsPage.qml
        InputsPage.qml
        ContainersPage.qml
        MessagePage.qml
        SheetPage.qml
)
target_link_libraries(turkitgallery PRIVATE Qt6::Core Qt6::Qml Qt6::Quick)
add_dependencies(turkitgallery turkit turkitpax)

qt_add_executable(tur-kit-gallery main.cpp)
target_compile_definitions(tur-kit-gallery PRIVATE TUR_VERSION="${PROJECT_VERSION}")
target_link_libraries(tur-kit-gallery PRIVATE Qt6::Core Qt6::Gui Qt6::Qml)
tur_link_qml_modules(tur-kit-gallery turkit turkitpax turkitgallery)

# The program starts, shows its window and reports no QML warning.
add_test(NAME gallery-starts COMMAND tur-kit-gallery --standalone --check)
set_tests_properties(gallery-starts PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
