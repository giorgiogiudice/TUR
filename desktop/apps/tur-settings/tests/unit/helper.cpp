// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for KDE System Settings, for the tests: it writes the arguments
// it was started with, as a JSON list of strings, to the file that the
// environment variable TUR_SETTINGS_HELPER_OUT names, and exits. A test then
// reads which module tur-settings asked System Settings to open.
//
// Plain C++: it is not part of the program and is not shipped.

#include <cstdio>
#include <cstdlib>
#include <string>

int main(int argc, char *argv[])
{
    const char *path = std::getenv("TUR_SETTINGS_HELPER_OUT");
    if (path == nullptr || path[0] == '\0')
        return 2;

    std::string text = "[";
    for (int i = 1; i < argc; ++i) {
        text += i > 1 ? ", \"" : "\"";
        for (const char *at = argv[i]; *at != '\0'; ++at) {
            if (*at == '"' || *at == '\\')
                text += '\\';
            text += *at;
        }
        text += '"';
    }
    text += "]\n";

    // Written under another name and moved into place, so that a test never
    // reads half a file.
    const std::string temporary = std::string(path) + ".part";
    std::FILE *file = std::fopen(temporary.c_str(), "w");
    if (file == nullptr)
        return 3;
    const bool written = std::fputs(text.c_str(), file) >= 0;
    if (std::fclose(file) != 0 || !written)
        return 3;
    return std::rename(temporary.c_str(), path) == 0 ? 0 : 3;
}
