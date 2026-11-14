<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 03: Night mode

Capture: `session-03.pcapng` (readable form: `session-03.txt`), 140 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 0.765 s**: did: switched to night mode
  - saw:

    ```text
    mode night
    ```

- **1.465 - 1.479 s**: did: asked which mode the gadget is in
  - saw:

    ```text
    night
    ```

- **2.179 - 2.196 s**: did: set the night-light level to 10
  - saw:

    ```text
    night-level 10
    ```

- **2.896 - 2.915 s**: did: asked for the night-light level
  - saw:

    ```text
    10
    ```

- **3.615 - 3.635 s**: did: set the night-light level to 3
  - saw:

    ```text
    night-level 3
    ```

- **4.335 - 4.855 s**: did: set the (daytime) display brightness to 55 while in night mode
  - saw:

    ```text
    brightness 55
    ```

- **5.555 - 6.071 s**: did: asked for the display brightness
  - saw:

    ```text
    55
    ```

- **6.772 - 6.790 s**: did: asked for the night-light level
  - saw:

    ```text
    3
    ```

- **7.490 - 7.755 s**: did: switched back to day mode
  - saw:

    ```text
    mode day
    ```

- **8.456 - 8.472 s**: did: asked which mode the gadget is in
  - saw:

    ```text
    day
    ```

