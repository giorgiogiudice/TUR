<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 08: Clearing the notes

Capture: `session-08.pcapng` (readable form: `session-08.txt`), 70 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 0.514 s**: did: stored the note 'temp' in slot 1
  - saw:

    ```text
    note 1 stored
    ```

- **1.215 - 1.231 s**: did: stored the note 'call Ana at 5' in slot 2
  - saw:

    ```text
    note 2 stored
    ```

- **1.932 - 1.946 s**: did: listed the stored notes
  - saw:

    ```text
    0 5
    1 4
    2 13
    7 96
    ```

- **2.647 - 2.660 s**: did: cleared all notes
  - saw:

    ```text
    notes cleared
    ```

- **3.361 - 3.373 s**: did: listed the stored notes (none left)
  - saw: nothing (no output)
