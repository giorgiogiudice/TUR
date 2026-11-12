# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The toolchain proof (task D-01): a one-page program, one QML test, one
# reference image. tur-probe is also what desktop/ci/probe.sh runs to settle
# the open assumptions of the design. A CI tool; not installed in the image.

qt_add_executable(tur-probe main.cpp)
qt_add_qml_module(tur-probe
    URI Tur.Probe
    VERSION 1.0
    NO_CACHEGEN
    QML_FILES
        Probe.qml
        BreezeProbe.qml
)
target_link_libraries(tur-probe PRIVATE Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2)
tur_link_qml_modules(tur-probe turkit)

# The program itself starts and draws its page without a QML warning.
add_test(NAME probe-program COMMAND tur-probe --standalone --strict --page Probe)
set_tests_properties(probe-program PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)

# The one QML test, run by the kit's test program.
add_test(NAME probe-qml COMMAND tst_kit -input ${CMAKE_CURRENT_SOURCE_DIR}/tst_probe.qml)
set_tests_properties(probe-qml PROPERTIES ENVIRONMENT "${TUR_TEST_ENVIRONMENT}" TIMEOUT 120)
