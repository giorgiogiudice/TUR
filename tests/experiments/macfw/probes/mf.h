/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw: the small framework every Objective-C probe includes.
 *
 * A probe is one Mach-O program with several groups, written by TUR, that
 * calls public Foundation and AppKit API and prints what it observed. It is
 * built once, on GitHub's macOS runner with Xcode's public toolchain, as a
 * universal binary; the same bytes then run on macOS (the oracle) and under
 * Darling on Linux (the candidate).
 *
 *     probe --list          the group names, one per line
 *     probe --run GROUP     one JSON line per observation, then a last line
 *
 *     {"id":"probe.group.check","k":"exact","v":"what was observed"}
 *     {"done":"probe.group"}
 *
 * "k" says how the differ compares the value:
 *     exact    byte for byte;
 *     num:T    numbers separated by commas, each within T of the oracle's;
 *     metric   depends on fonts, theme or screen (a title bar's height, a
 *              label's natural width): reported, compared within 10%, never
 *              counted as a difference on its own;
 *     env      describes the machine (screen size, OS version): reported
 *              only.
 *
 * Values are written so that two machines that behave the same print the
 * same bytes: numbers with one decimal, class names of public classes only,
 * booleans as YES/NO, orders as a list. Never pointers, times of day,
 * process identifiers or private class names.
 *
 * An exception inside one check is caught and printed as that check's value
 * ("!exception NSInvalidArgumentException"), so an unimplemented method costs
 * one observation, not the group. A crash ends the group; the runner records
 * it from the exit status.
 *
 * Apple's frameworks are observed only from outside, through their public,
 * documented API. Nothing here was written from Apple source, disassembly or
 * any other restricted material.
 */
#ifndef MF_H
#define MF_H

#import <Foundation/Foundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void (*mf_group_fn)(void);

typedef struct {
    const char *name;
    mf_group_fn fn;
} mf_group;

static const char *mf_prog = "?";
static const char *mf_cur = "?";

static NSString *mf_json_escape(NSString *s)
{
    NSMutableString *o = [NSMutableString stringWithCapacity:s.length + 8];
    for (NSUInteger i = 0; i < s.length; i++) {
        unichar c = [s characterAtIndex:i];
        if (c == '"' || c == '\\') [o appendFormat:@"\\%C", c];
        else if (c < 0x20 || c == 0x7f) [o appendFormat:@"\\u%04x", c];
        else [o appendFormat:@"%C", c];
    }
    return o;
}

static void mf_emit(const char *check, const char *kind, NSString *value)
{
    if (value == nil) value = @"(nil)";
    NSString *line = [NSString stringWithFormat:@"{\"id\":\"%s.%s.%s\",\"k\":\"%s\",\"v\":\"%@\"}\n",
                      mf_prog, mf_cur, check, kind, mf_json_escape(value)];
    fputs(line.UTF8String, stdout);
    fflush(stdout);
}

/* Formatting helpers: the same bytes for the same value everywhere. */
static NSString *mf_b(BOOL b) { return b ? @"YES" : @"NO"; }
static NSString *mf_f(double d) { return [NSString stringWithFormat:@"%.1f", d]; }
static NSString *mf_pt(NSPoint p) { return [NSString stringWithFormat:@"%.1f,%.1f", p.x, p.y]; }
static NSString *mf_sz(NSSize s) { return [NSString stringWithFormat:@"%.1f,%.1f", s.width, s.height]; }
static NSString *mf_rect(NSRect r)
{
    return [NSString stringWithFormat:@"%.1f,%.1f,%.1f,%.1f",
            r.origin.x, r.origin.y, r.size.width, r.size.height];
}
static NSString *mf_range(NSRange r)
{
    if (r.location == NSNotFound) return [NSString stringWithFormat:@"NotFound,%lu", (unsigned long)r.length];
    return [NSString stringWithFormat:@"%lu,%lu", (unsigned long)r.location, (unsigned long)r.length];
}
static NSString *mf_cls(id o) { return o ? NSStringFromClass([o class]) : @"(nil)"; }

/* A check: evaluates the expression (an NSString *) with exceptions caught.
 * Variadic because the preprocessor splits arguments at the commas inside
 * Objective-C message brackets and literals. */
#define MF_CHECK_K(name, kind, ...)                                               \
    do {                                                                          \
        NSString *mf_v_;                                                          \
        @try { mf_v_ = (__VA_ARGS__); }                                           \
        @catch (NSException *mf_e_) { mf_v_ = [@"!exception " stringByAppendingString:mf_e_.name]; } \
        mf_emit((name), (kind), mf_v_);                                           \
    } while (0)
#define MF_CHECK(name, ...) MF_CHECK_K(name, "exact", __VA_ARGS__)
#define MF_METRIC(name, ...) MF_CHECK_K(name, "metric", __VA_ARGS__)
#define MF_ENV(name, ...) MF_CHECK_K(name, "env", __VA_ARGS__)

/* Records the order in which callbacks arrive; a check prints it joined. */
static NSMutableArray<NSString *> *mf_log;
static void mf_log_reset(void) { mf_log = [NSMutableArray array]; }
static void mf_log_add(NSString *s) { if (mf_log) [mf_log addObject:s]; }
static NSString *mf_log_str(void) { return mf_log.count ? [mf_log componentsJoinedByString:@" "] : @"(none)"; }

/* Lets queued work and notifications run, without waiting on wall-clock
 * events: a fixed number of run-loop turns, each bounded. */
static void mf_spin(double seconds)
{
    NSDate *until = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while ([until timeIntervalSinceNow] > 0) {
        @autoreleasepool {
            [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode
                                     beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
        }
    }
}

static void mf_uncaught(NSException *e)
{
    mf_emit("__uncaught", "exact", [NSString stringWithFormat:@"%@", e.name]);
}

static int mf_main(const char *prog, int argc, char **argv, const mf_group *groups, size_t n)
{
    mf_prog = prog;
    NSSetUncaughtExceptionHandler(mf_uncaught);
    if (argc == 2 && strcmp(argv[1], "--list") == 0) {
        for (size_t i = 0; i < n; i++) printf("%s\n", groups[i].name);
        return 0;
    }
    if (argc == 3 && strcmp(argv[1], "--run") == 0) {
        for (size_t i = 0; i < n; i++) {
            if (strcmp(argv[2], groups[i].name) != 0) continue;
            mf_cur = groups[i].name;
            @autoreleasepool { groups[i].fn(); }
            printf("{\"done\":\"%s.%s\"}\n", prog, mf_cur);
            fflush(stdout);
            return 0;
        }
        fprintf(stderr, "%s: no group %s\n", prog, argv[2]);
        return 2;
    }
    fprintf(stderr, "usage: %s --list | --run GROUP\n", prog);
    return 2;
}

#define MF_GROUPS(...) static const mf_group mf_groups_[] = { __VA_ARGS__ }
#define MF_MAIN(prog)                                                              \
    int main(int argc, char **argv)                                                \
    {                                                                              \
        @autoreleasepool {                                                         \
            return mf_main(prog, argc, argv, mf_groups_,                           \
                           sizeof(mf_groups_) / sizeof(mf_groups_[0]));            \
        }                                                                          \
    }

#endif
