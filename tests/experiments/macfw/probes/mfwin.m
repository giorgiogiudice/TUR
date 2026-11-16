/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw helper `mfwin`, macOS side only: lists the on-screen windows of one
 * process as JSON lines, for the ladder's "usable window" check on the
 * oracle. Bounds, layer and alpha need no permission; window titles do
 * (Screen Recording) and are not used. The Linux side gets the same facts
 * from the X server (harness/ladder.py).
 *
 *     mfwin PID     {"wid":N,"layer":L,"alpha":A,"bounds":"x,y,w,h"} per window
 */
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
    @autoreleasepool {
        if (argc != 2) {
            fprintf(stderr, "usage: mfwin PID\n");
            return 2;
        }
        pid_t pid = (pid_t)atoi(argv[1]);
        CFArrayRef list = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                                                     kCGNullWindowID);
        if (!list) return 1;
        for (NSDictionary *w in (__bridge NSArray *)list) {
            if ([w[(__bridge NSString *)kCGWindowOwnerPID] intValue] != pid) continue;
            CGRect r;
            if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)w[(__bridge NSString *)kCGWindowBounds], &r))
                continue;
            printf("{\"wid\":%u,\"layer\":%d,\"alpha\":%.2f,\"bounds\":\"%.0f,%.0f,%.0f,%.0f\"}\n",
                   [w[(__bridge NSString *)kCGWindowNumber] unsignedIntValue],
                   [w[(__bridge NSString *)kCGWindowLayer] intValue],
                   [w[(__bridge NSString *)kCGWindowAlpha] doubleValue],
                   r.origin.x, r.origin.y, r.size.width, r.size.height);
        }
        CFRelease(list);
    }
    return 0;
}
