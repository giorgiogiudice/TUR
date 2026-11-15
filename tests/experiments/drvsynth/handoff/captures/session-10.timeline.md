<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 10: Frost alert and a last picture

Capture: `session-10.pcapng` (readable form: `session-10.txt`), 160 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.501 - 1.021 s**: did: set the temperature alert to -5.0 degrees
  - saw:

    ```text
    alert -5.0
    ```

- **1.721 - 2.241 s**: did: asked for the temperature alert
  - saw:

    ```text
    -5.0
    ```

- **2.942 - 8.979 s**: did: watched the gadget for 6 seconds
  - saw:

    ```text
    alert -3.25
    ```

- **4.993 s**: the gadget (outdoors) warmed up past the alert temperature
- **9.680 - 10.199 s**: did: set the temperature alert to 30 degrees
  - saw:

    ```text
    alert 30.0
    ```

- **10.900 - 11.416 s**: did: asked for the temperature alert
  - saw:

    ```text
    30.0
    ```

- **12.117 - 12.637 s**: did: switched the temperature alert off
  - saw:

    ```text
    alert off
    ```

- **13.338 - 13.354 s**: did: showed the letters 'TUR' with a line under them
  - the picture (16 x 8 pixels; `.` is off, `1`-`f` are brightness levels, `f` brightest):

    ```text
    fff.f..f.fff....
    .f..f..f.f..f...
    .f..f..f.fff....
    .f..f..f.f.f....
    .f...ff..f..f...
    ................
    5555555555555555
    ................
    ```

  - saw:

    ```text
    image shown
    ```

- **14.054 - 14.067 s**: did: asked for the display brightness
  - saw:

    ```text
    20
    ```

