<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 05: Notes

Capture: `session-05.pcapng` (readable form: `session-05.txt`), 152 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 0.513 s**: did: listed the stored notes (none)
  - saw: nothing (no output)
- **1.213 - 1.233 s**: did: stored the note 'hello' in slot 0
  - saw:

    ```text
    note 0 stored
    ```

- **1.933 - 1.952 s**: did: stored the note 'Buy milk and eggs' in slot 3
  - saw:

    ```text
    note 3 stored
    ```

- **2.653 - 2.666 s**: did: listed the stored notes
  - saw:

    ```text
    0 5
    3 17
    ```

- **3.367 - 3.378 s**: did: read the note in slot 0
  - saw:

    ```text
    hello
    ```

- **4.079 - 4.094 s**: did: read the note in slot 3
  - saw:

    ```text
    Buy milk and eggs
    ```

- **4.794 - 4.811 s**: did: stored a long note in slot 7: 'The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs! 0123456789'
  - saw:

    ```text
    note 7 stored
    ```

- **5.511 - 5.526 s**: did: read the note in slot 7
  - saw:

    ```text
    The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs! 0123456789
    ```

- **6.227 - 6.241 s**: did: tried to read slot 5, which is empty
  - saw:

    ```text
    empty
    ```

- **6.942 - 6.954 s**: did: deleted the note in slot 3
  - saw:

    ```text
    note 3 deleted
    ```

- **7.655 - 7.668 s**: did: listed the stored notes
  - saw:

    ```text
    0 5
    7 96
    ```

