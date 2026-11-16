// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
package org.tur.probe;

/** The native half of the benchmark (jni/probe.c, libturprobe.so). */
final class Native {
    private Native() {
    }

    static native String abi();

    static native long benchInt(int rounds);

    static native double benchFp(int rounds);
}
