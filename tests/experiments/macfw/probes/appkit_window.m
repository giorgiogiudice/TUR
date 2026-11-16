/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw probe `win`: NSApplication and NSWindow as an application meets them
 * on its way to a first window: defaults per style, the frame and content
 * rectangle arithmetic, the order of delegate calls and notifications when a
 * window is shown, made key, resized, miniaturised and closed, and what the
 * screen reports. Written from Apple's public documentation only.
 *
 * Needs a display (on Linux: Xvfb with a window manager, or a headless
 * Wayland compositor); each group shows its windows for a moment and closes
 * them.
 */
#import <AppKit/AppKit.h>
#import "mf.h"

@interface MFWindowDelegate : NSObject <NSWindowDelegate>
@end

@implementation MFWindowDelegate
- (void)windowDidBecomeKey:(NSNotification *)n { mf_log_add(@"didBecomeKey"); }
- (void)windowDidResignKey:(NSNotification *)n { mf_log_add(@"didResignKey"); }
- (void)windowDidBecomeMain:(NSNotification *)n { mf_log_add(@"didBecomeMain"); }
- (void)windowDidResignMain:(NSNotification *)n { mf_log_add(@"didResignMain"); }
- (void)windowDidResize:(NSNotification *)n { mf_log_add(@"didResize"); }
- (void)windowDidMove:(NSNotification *)n { mf_log_add(@"didMove"); }
- (NSSize)windowWillResize:(NSWindow *)w toSize:(NSSize)s
{
    mf_log_add([NSString stringWithFormat:@"willResize(%@)", mf_sz(s)]);
    return s;
}
- (BOOL)windowShouldClose:(NSWindow *)w { mf_log_add(@"shouldClose"); return YES; }
- (void)windowWillClose:(NSNotification *)n { mf_log_add(@"willClose"); }
- (void)windowDidMiniaturize:(NSNotification *)n { mf_log_add(@"didMiniaturize"); }
- (void)windowDidDeminiaturize:(NSNotification *)n { mf_log_add(@"didDeminiaturize"); }
@end

static NSWindow *make_window(NSWindowStyleMask style)
{
    NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(200, 200, 400, 300)
                                              styleMask:style
                                                backing:NSBackingStoreBuffered
                                                  defer:YES];
    w.releasedWhenClosed = NO;
    return w;
}

static void start_app(void)
{
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp finishLaunching];
    [NSApp activateIgnoringOtherApps:YES];
}

static void g_app(void)
{
    MF_CHECK("before_shared", mf_b(NSApp != nil));
    [NSApplication sharedApplication];
    MF_CHECK("shared_class", mf_cls(NSApp));
    MF_CHECK("policy_default", [NSString stringWithFormat:@"%ld", (long)NSApp.activationPolicy]);
    MF_CHECK("set_policy_regular", mf_b([NSApp setActivationPolicy:NSApplicationActivationPolicyRegular]));
    MF_CHECK("main_menu_before", mf_cls(NSApp.mainMenu));
    MF_CHECK("windows_before", [NSString stringWithFormat:@"%lu", (unsigned long)NSApp.windows.count]);
    [NSApp finishLaunching];
    MF_CHECK("running_after_finish", mf_b(NSApp.isRunning));
    MF_CHECK("delegate_default", mf_cls(NSApp.delegate));
    MF_CHECK("key_window_none", mf_cls(NSApp.keyWindow));
    MF_ENV("appearance", NSApp.effectiveAppearance.name);
}

static void g_styles(void)
{
    [NSApplication sharedApplication];
    struct { const char *name; NSWindowStyleMask mask; } styles[] = {
        { "borderless", NSWindowStyleMaskBorderless },
        { "titled", NSWindowStyleMaskTitled },
        { "standard", NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable |
                      NSWindowStyleMaskResizable },
        { "utility_panel", NSWindowStyleMaskTitled | NSWindowStyleMaskUtilityWindow },
        { "fullsize_content", NSWindowStyleMaskTitled | NSWindowStyleMaskFullSizeContentView },
    };
    for (size_t i = 0; i < sizeof(styles) / sizeof(styles[0]); i++) {
        BOOL panel = (styles[i].mask & NSWindowStyleMaskUtilityWindow) != 0;
        /* Not made with make_window: the default of releasedWhenClosed is
         * one of the observations. None of these windows is closed. */
        Class cls = panel ? [NSPanel class] : [NSWindow class];
        NSWindow *w = [[cls alloc] initWithContentRect:NSMakeRect(200, 200, 400, 300)
                                             styleMask:styles[i].mask
                                               backing:NSBackingStoreBuffered
                                                 defer:YES];
        char n[96];
#define S(suffix) (snprintf(n, sizeof n, "%s.%s", styles[i].name, suffix), n)
        MF_CHECK(S("mask"), [NSString stringWithFormat:@"%lu", (unsigned long)w.styleMask]);
        MF_CHECK(S("content_size"), mf_sz(w.contentView.frame.size));
        MF_CHECK(S("content_view_class"), mf_cls(w.contentView));
        MF_METRIC(S("frame"), mf_rect(w.frame));
        MF_METRIC(S("frame_for_content"), mf_rect([NSWindow frameRectForContentRect:NSMakeRect(0, 0, 400, 300)
                                                                          styleMask:styles[i].mask]));
        MF_CHECK(S("content_roundtrip"), mf_rect([w contentRectForFrameRect:[w frameRectForContentRect:NSMakeRect(10, 20, 400, 300)]]));
        MF_CHECK(S("title"), w.title);
        MF_CHECK(S("visible"), mf_b(w.isVisible));
        MF_CHECK(S("opaque"), mf_b(w.isOpaque));
        MF_CHECK(S("has_shadow"), mf_b(w.hasShadow));
        MF_CHECK(S("level"), [NSString stringWithFormat:@"%ld", (long)w.level]);
        MF_CHECK(S("can_key"), mf_b(w.canBecomeKeyWindow));
        MF_CHECK(S("can_main"), mf_b(w.canBecomeMainWindow));
        MF_CHECK(S("released_when_closed"), mf_b(w.isReleasedWhenClosed));
        MF_CHECK(S("hides_on_deactivate"), mf_b(w.hidesOnDeactivate));
        MF_CHECK(S("buttons"), [NSString stringWithFormat:@"%@%@%@",
                                [w standardWindowButton:NSWindowCloseButton] ? @"c" : @"-",
                                [w standardWindowButton:NSWindowMiniaturizeButton] ? @"m" : @"-",
                                [w standardWindowButton:NSWindowZoomButton] ? @"z" : @"-"]);
        MF_CHECK(S("min_size"), mf_sz(w.minSize));
        MF_CHECK(S("content_min_size"), mf_sz(w.contentMinSize));
        MF_CHECK(S("collection_behavior"), [NSString stringWithFormat:@"%lu", (unsigned long)w.collectionBehavior]);
        MF_CHECK(S("first_responder"), mf_cls(w.firstResponder));
        MF_CHECK(S("initial_first_responder"), mf_cls(w.initialFirstResponder));
#undef S
    }
}

static void g_show(void)
{
    start_app();
    MFWindowDelegate *del = [MFWindowDelegate new];
    NSWindow *w = make_window(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable |
                              NSWindowStyleMaskMiniaturizable);
    w.delegate = del;
    w.title = @"macfw";
    mf_log_reset();
    [w makeKeyAndOrderFront:nil];
    mf_spin(0.5);
    MF_CHECK("show_sequence", mf_log_str());
    MF_CHECK("visible", mf_b(w.isVisible));
    MF_CHECK("is_key", mf_b(w.isKeyWindow));
    MF_CHECK("is_main", mf_b(w.isMainWindow));
    MF_CHECK("app_key_is_it", mf_b(NSApp.keyWindow == w));
    MF_CHECK("in_windows", mf_b([NSApp.windows containsObject:w]));
    MF_CHECK("on_screen", mf_b(w.isOnActiveSpace && w.screen != nil));
    MF_CHECK("window_number_positive", mf_b(w.windowNumber > 0));
    MF_CHECK("content_size_after_show", mf_sz(w.contentView.frame.size));

    mf_log_reset();
    [w setContentSize:NSMakeSize(500, 350)];
    mf_spin(0.3);
    MF_CHECK("set_content_size_sequence", mf_log_str());
    MF_CHECK("content_size_after_set", mf_sz(w.contentView.frame.size));

    mf_log_reset();
    NSRect f = w.frame;
    f.origin.x += 30;
    [w setFrame:f display:YES];
    mf_spin(0.3);
    MF_CHECK("move_sequence", mf_log_str());
    MF_CHECK("origin_x_moved", mf_b(fabs(w.frame.origin.x - f.origin.x) < 0.5));

    mf_log_reset();
    [w center];
    mf_spin(0.2);
    NSRect vf = w.screen.visibleFrame;
    MF_CHECK("center_horizontal", mf_b(fabs(NSMidX(w.frame) - NSMidX(vf)) <= 1.0));
    MF_CHECK("center_upper_third", mf_b(NSMidY(w.frame) >= NSMidY(vf) - 1.0));

    mf_log_reset();
    [w performClose:nil];
    mf_spin(0.3);
    MF_CHECK("close_sequence", mf_log_str());
    MF_CHECK("visible_after_close", mf_b(w.isVisible));
    MF_CHECK("key_after_close", mf_cls(NSApp.keyWindow));
}

static void g_two_windows(void)
{
    start_app();
    MFWindowDelegate *d1 = [MFWindowDelegate new], *d2 = [MFWindowDelegate new];
    NSWindow *a = make_window(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable);
    NSWindow *b = make_window(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable);
    a.delegate = d1;
    b.delegate = d2;
    [a makeKeyAndOrderFront:nil];
    mf_spin(0.3);
    mf_log_reset();
    [b makeKeyAndOrderFront:nil];
    mf_spin(0.4);
    MF_CHECK("second_key_sequence", mf_log_str());
    MF_CHECK("a_key", mf_b(a.isKeyWindow));
    MF_CHECK("b_key", mf_b(b.isKeyWindow));
    MF_CHECK("ordered_front_first", mf_b(NSApp.orderedWindows.firstObject == b));
    mf_log_reset();
    [b orderOut:nil];
    mf_spin(0.4);
    MF_CHECK("order_out_sequence", mf_log_str());
    MF_CHECK("a_key_after_order_out", mf_b(a.isKeyWindow));
    [a close];
}

static void g_screen(void)
{
    [NSApplication sharedApplication];
    MF_CHECK("has_main_screen", mf_b(NSScreen.mainScreen != nil));
    MF_CHECK("screens_at_least_one", mf_b(NSScreen.screens.count >= 1));
    MF_CHECK("main_is_first_or_key", mf_b(NSScreen.screens.firstObject != nil));
    MF_ENV("main_frame", mf_rect(NSScreen.mainScreen.frame));
    MF_ENV("main_visible", mf_rect(NSScreen.mainScreen.visibleFrame));
    MF_ENV("scale", mf_f(NSScreen.mainScreen.backingScaleFactor));
    MF_CHECK("visible_inside_frame", mf_b(NSContainsRect(NSScreen.mainScreen.frame, NSScreen.mainScreen.visibleFrame)));
    MF_CHECK("first_screen_origin", mf_pt(NSScreen.screens.firstObject.frame.origin));
    MF_CHECK("device_description_has_number",
             mf_b(NSScreen.mainScreen.deviceDescription[@"NSScreenNumber"] != nil));
}

MF_GROUPS({ "app", g_app }, { "styles", g_styles }, { "show", g_show }, { "two_windows", g_two_windows },
          { "screen", g_screen });
MF_MAIN("win")
