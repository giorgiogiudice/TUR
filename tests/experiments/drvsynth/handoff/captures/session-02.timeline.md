<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 02: Brightness

Capture: `session-02.pcapng` (readable form: `session-02.txt`), 112 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 0.514 s**: did: set the display brightness to 40
  - saw:

    ```text
    brightness 40
    ```

- **1.215 - 1.228 s**: did: asked for the display brightness
  - saw:

    ```text
    40
    ```

- **1.929 - 1.945 s**: did: set the display brightness to 41
  - saw:

    ```text
    brightness 41
    ```

- **2.645 - 2.663 s**: did: set the display brightness to 75
  - saw:

    ```text
    brightness 75
    ```

- **3.363 - 3.381 s**: did: set the display brightness to 0
  - saw:

    ```text
    brightness 0
    ```

- **4.082 - 4.094 s**: did: set the display brightness to 100
  - saw:

    ```text
    brightness 100
    ```

- **4.795 - 4.807 s**: did: set the display brightness to 7
  - saw:

    ```text
    brightness 7
    ```

- **5.508 - 5.526 s**: did: asked for the display brightness
  - saw:

    ```text
    7
    ```

