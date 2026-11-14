<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 06: Pictures

Capture: `session-06.pcapng` (readable form: `session-06.txt`), 100 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 0.513 s**: did: showed a smiley on the display
  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):

    ```text
    ....ffffffff....
    ..ff........ff..
    .f..ff....ff..f.
    f...ff....ff...f
    f..............f
    f..8........8..f
    .f..88888888..f.
    ..ff........ff..
    ```

  - saw:

    ```text
    image shown
    ```

- **1.214 - 1.234 s**: did: showed a diagonal line
  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):

    ```text
    ff..............
    ..ff............
    ....ff..........
    ......ff........
    ........ff......
    ..........ff....
    ............ff..
    ..............ff
    ```

  - saw:

    ```text
    image shown
    ```

- **1.935 - 1.947 s**: did: showed a left-to-right brightness ramp
  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):

    ```text
    .123456789abcdef
    .123456789abcdef
    .123456789abcdef
    .123456789abcdef
    .123456789abcdef
    .123456789abcdef
    .123456789abcdef
    .123456789abcdef
    ```

  - saw:

    ```text
    image shown
    ```

- **2.648 - 2.665 s**: did: lit only the top-left pixel, fully
  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):

    ```text
    f...............
    ................
    ................
    ................
    ................
    ................
    ................
    ................
    ```

  - saw:

    ```text
    image shown
    ```

- **3.365 - 3.379 s**: did: showed a pattern of brightness steps
  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):

    ```text
    .1.1.1.1.1.1.1.1
    2323232323232323
    4545454545454545
    6767676767676767
    8989898989898989
    abababababababab
    cdcdcdcdcdcdcdcd
    efefefefefefefef
    ```

  - saw:

    ```text
    image shown
    ```

- **4.079 - 4.095 s**: did: cleared the display
  - saw:

    ```text
    display cleared
    ```

