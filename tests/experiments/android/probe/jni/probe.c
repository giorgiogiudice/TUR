// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The native half of the probe's benchmark: the same integer and
// floating-point work as ProbeActivity.benchInt and benchFp, so that the
// checksums can be compared and the times set side by side. Built for
// x86_64 and arm64-v8a by build.sh.
#include <jni.h>
#include <stdint.h>

#if defined(__aarch64__)
#define TUR_ABI "arm64-v8a"
#elif defined(__x86_64__)
#define TUR_ABI "x86_64"
#elif defined(__arm__)
#define TUR_ABI "armeabi-v7a"
#elif defined(__i386__)
#define TUR_ABI "x86"
#else
#define TUR_ABI "unknown"
#endif

JNIEXPORT jstring JNICALL Java_org_tur_probe_Native_abi(JNIEnv *env, jclass cls)
{
	(void)cls;
	return (*env)->NewStringUTF(env, TUR_ABI);
}

JNIEXPORT jlong JNICALL Java_org_tur_probe_Native_benchInt(JNIEnv *env, jclass cls, jint rounds)
{
	static uint8_t buf[65536];
	uint64_t x = 88172645463325252ULL;
	uint64_t h = 0xcbf29ce484222325ULL;
	(void)env;
	(void)cls;
	for (int i = 0; i < 65536; i++) {
		x ^= x << 13;
		x ^= x >> 7;
		x ^= x << 17;
		buf[i] = (uint8_t)x;
	}
	for (int r = 0; r < rounds; r++) {
		for (int i = 0; i < 65536; i++) {
			h ^= buf[i];
			h *= 0x100000001b3ULL;
		}
		buf[r & 0xffff] = (uint8_t)h;
	}
	return (jlong)h;
}

JNIEXPORT jdouble JNICALL Java_org_tur_probe_Native_benchFp(JNIEnv *env, jclass cls, jint rounds)
{
	enum { N = 96 };
	static double a[N * N], b[N * N], c[N * N];
	double s = 0.0;
	(void)env;
	(void)cls;
	for (int i = 0; i < N * N; i++) {
		a[i] = (i % 7) * 0.5 + 1.0;
		b[i] = (i % 5) * 0.25 + 0.5;
	}
	for (int r = 0; r < rounds; r++) {
		for (int i = 0; i < N; i++) {
			for (int j = 0; j < N; j++) {
				double t = 0.0;
				for (int k = 0; k < N; k++)
					t += a[i * N + k] * b[k * N + j];
				c[i * N + j] = t;
			}
		}
		s += c[r % (N * N)];
		a[r % (N * N)] += 1.0;
	}
	return s;
}
