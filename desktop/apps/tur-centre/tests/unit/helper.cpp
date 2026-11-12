// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for tur-catalogue, for the tests of ProcessSource. It is given
// the arguments tur-catalogue is given, and the last of them (the subject)
// says how to behave:
//
//   sleep      never answers
//   flood      prints without end
//   crash      ends abnormally
//   garbage    prints text that is not JSON
//   list       prints JSON that is not an object
//   silent     prints nothing, and ends with status 1
//   refused    reports a catalogue error on standard error, as tur-catalogue
//              does for a catalogue it cannot load
//   noisy      prints an answer, and a great deal on standard error
//   input      prints how many bytes it could read from standard input
//   anything else: prints its arguments as a JSON document
//
// Plain C++ with nothing from Qt, so that it cannot be mistaken for part of
// the program.

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

namespace {

std::string quoted(const std::string &text)
{
    std::string out = "\"";
    for (const char character : text) {
        if (character == '"' || character == '\\')
            out += '\\';
        out += character;
    }
    return out + "\"";
}

} // namespace

int main(int argc, char *argv[])
{
    const std::string subject = argc > 1 ? argv[argc - 1] : "";

    if (subject == "sleep") {
        std::this_thread::sleep_for(std::chrono::seconds(120));
        return 0;
    }
    if (subject == "flood") {
        const std::string line(1024, 'x');
        for (;;) {
            if (std::fputs(line.c_str(), stdout) < 0)
                return 0;
        }
    }
    if (subject == "crash")
        std::abort();
    if (subject == "garbage") {
        std::puts("this is not JSON");
        return 0;
    }
    if (subject == "list") {
        std::puts("[1, 2, 3]");
        return 0;
    }
    if (subject == "silent")
        return 1;
    if (subject == "refused") {
        std::fputs("error[TUR-DESKTOP-CATALOGUE-001]: cannot read the catalogue directory /nowhere: it does not exist\n",
                   stderr);
        return 1;
    }
    if (subject == "noisy") {
        const std::string line(1023, 'e');
        for (int i = 0; i < 256; ++i)
            std::fprintf(stderr, "%s\n", line.c_str());
        std::puts("{\"query\": \"noisy\", \"hits\": []}");
        return 6;
    }
    if (subject == "input") {
        long bytes = 0;
        while (std::fgetc(stdin) != EOF)
            ++bytes;
        std::printf("{\"input_bytes\": %ld}\n", bytes);
        return 0;
    }

    std::string out = "{\"arguments\": [";
    for (int i = 1; i < argc; ++i) {
        if (i > 1)
            out += ", ";
        out += quoted(argv[i]);
    }
    out += "]}";
    std::puts(out.c_str());
    return 0;
}
