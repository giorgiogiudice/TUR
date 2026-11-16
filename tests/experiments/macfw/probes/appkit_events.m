/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw probe `evt`: the responder chain and event dispatch, driven with
 * events the probe makes itself and hands to -[NSWindow sendEvent:] and
 * -[NSApplication sendEvent:]: which view receives a key or mouse event,
 * what the chain of next responders is, how first responder changes are
 * negotiated, and where an action sent to the first responder ends up.
 * No input device or accessibility permission is needed. Written from
 * Apple's public documentation only.
 */
#import <AppKit/AppKit.h>
#import "mf.h"

@interface MFResponderView : NSView
@property (copy) NSString *mfName; /* not `tag`: NSView has an integer tag */
@property BOOL accepts;
@end

@implementation MFResponderView
- (BOOL)acceptsFirstResponder { mf_log_add([NSString stringWithFormat:@"%@.acceptsFirstResponder", self.mfName]); return self.accepts; }
- (BOOL)becomeFirstResponder { mf_log_add([NSString stringWithFormat:@"%@.become", self.mfName]); return YES; }
- (BOOL)resignFirstResponder { mf_log_add([NSString stringWithFormat:@"%@.resign", self.mfName]); return YES; }
- (void)keyDown:(NSEvent *)e { mf_log_add([NSString stringWithFormat:@"%@.keyDown(%@)", self.mfName, e.characters]); [super keyDown:e]; }
- (void)mouseDown:(NSEvent *)e
{
    NSPoint p = [self convertPoint:e.locationInWindow fromView:nil];
    mf_log_add([NSString stringWithFormat:@"%@.mouseDown(%@,clicks=%ld)", self.mfName, mf_pt(p), (long)e.clickCount]);
}
- (void)mouseUp:(NSEvent *)e { mf_log_add([NSString stringWithFormat:@"%@.mouseUp", self.mfName]); }
- (void)rightMouseDown:(NSEvent *)e { mf_log_add([NSString stringWithFormat:@"%@.rightMouseDown", self.mfName]); [super rightMouseDown:e]; }
- (void)scrollWheel:(NSEvent *)e { mf_log_add([NSString stringWithFormat:@"%@.scrollWheel", self.mfName]); [super scrollWheel:e]; }
- (void)flagsChanged:(NSEvent *)e { mf_log_add([NSString stringWithFormat:@"%@.flagsChanged", self.mfName]); }
- (BOOL)acceptsFirstMouse:(NSEvent *)e { return YES; }
- (void)customAction:(id)sender { mf_log_add([NSString stringWithFormat:@"%@.customAction", self.mfName]); }
@end

@interface MFWindowWithLog : NSWindow
@end
@implementation MFWindowWithLog
- (void)keyDown:(NSEvent *)e { mf_log_add(@"window.keyDown"); [super keyDown:e]; }
- (void)noResponderFor:(SEL)s { mf_log_add([NSString stringWithFormat:@"window.noResponderFor(%@)", NSStringFromSelector(s)]); }
@end

@interface MFAppDelegate : NSObject <NSApplicationDelegate>
@end
@implementation MFAppDelegate
- (void)customAction:(id)sender { mf_log_add(@"appDelegate.customAction"); }
@end

static MFWindowWithLog *win;
static MFResponderView *outer, *inner, *other;

static void setup(void)
{
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp finishLaunching];
    win = [[MFWindowWithLog alloc] initWithContentRect:NSMakeRect(100, 100, 300, 200)
                                             styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    win.releasedWhenClosed = NO;
    outer = [[MFResponderView alloc] initWithFrame:NSMakeRect(0, 0, 200, 150)];
    outer.mfName = @"outer";
    outer.accepts = YES;
    inner = [[MFResponderView alloc] initWithFrame:NSMakeRect(20, 20, 50, 40)];
    inner.mfName = @"inner";
    inner.accepts = YES;
    other = [[MFResponderView alloc] initWithFrame:NSMakeRect(220, 10, 60, 60)];
    other.mfName = @"other";
    other.accepts = NO;
    [win.contentView addSubview:outer];
    [outer addSubview:inner];
    [win.contentView addSubview:other];
    [win makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
    mf_spin(0.4);
}

static NSString *chain(NSResponder *r)
{
    NSMutableArray *o = [NSMutableArray array];
    for (int i = 0; r && i < 12; r = r.nextResponder, i++) {
        if ([r isKindOfClass:[MFResponderView class]]) [o addObject:((MFResponderView *)r).mfName];
        else if (r == win.contentView) [o addObject:@"contentView"];
        else if (r == win) [o addObject:@"window"];
        else if (r == NSApp) [o addObject:@"app"];
        else [o addObject:[NSString stringWithFormat:@"other:%@", [r isKindOfClass:[NSViewController class]] ? @"controller" : @"?"]];
    }
    return [o componentsJoinedByString:@">"];
}

static NSEvent *key(NSString *c)
{
    return [NSEvent keyEventWithType:NSEventTypeKeyDown location:NSZeroPoint modifierFlags:0 timestamp:0
                        windowNumber:win.windowNumber context:nil characters:c charactersIgnoringModifiers:c
                           isARepeat:NO keyCode:0];
}

static NSEvent *mouse(NSEventType t, NSPoint p, NSInteger clicks)
{
    return [NSEvent mouseEventWithType:t location:p modifierFlags:0 timestamp:0 windowNumber:win.windowNumber
                               context:nil eventNumber:0 clickCount:clicks pressure:1];
}

static void g_chain(void)
{
    setup();
    MF_CHECK("initial_first_responder", win.firstResponder == win ? @"window" : mf_cls(win.firstResponder));
    MF_CHECK("chain_inner", chain(inner));
    MF_CHECK("window_next", mf_cls(win.nextResponder));
    mf_log_reset();
    MF_CHECK("make_inner", [NSString stringWithFormat:@"%@ %@", mf_b([win makeFirstResponder:inner]), mf_log_str()]);
    mf_log_reset();
    MF_CHECK("make_other_refuses", [NSString stringWithFormat:@"%@ %@", mf_b([win makeFirstResponder:other]), mf_log_str()]);
    MF_CHECK("still_inner", mf_b(win.firstResponder == inner));
    mf_log_reset();
    MF_CHECK("make_nil", [NSString stringWithFormat:@"%@ %@ now=%@", mf_b([win makeFirstResponder:nil]), mf_log_str(),
                          win.firstResponder == win ? @"window" : mf_cls(win.firstResponder)]);
    [win close];
}

static void g_keys(void)
{
    setup();
    [win makeFirstResponder:inner];
    mf_log_reset();
    [win sendEvent:key(@"a")];
    mf_spin(0.1);
    MF_CHECK("key_to_inner", mf_log_str());
    inner.accepts = NO;
    [win makeFirstResponder:outer];
    mf_log_reset();
    [NSApp sendEvent:key(@"b")];
    mf_spin(0.1);
    MF_CHECK("app_send_key_to_outer", mf_log_str());
    mf_log_reset();
    [win makeFirstResponder:nil];
    [win sendEvent:key(@"c")];
    mf_spin(0.1);
    MF_CHECK("key_no_view", mf_log_str());
    mf_log_reset();
    [win sendEvent:[NSEvent keyEventWithType:NSEventTypeFlagsChanged location:NSZeroPoint
                               modifierFlags:NSEventModifierFlagShift timestamp:0 windowNumber:win.windowNumber
                                     context:nil characters:@"" charactersIgnoringModifiers:@"" isARepeat:NO keyCode:56]];
    mf_spin(0.1);
    MF_CHECK("flags_changed_no_view", mf_log_str());
    [win close];
}

static void g_mouse(void)
{
    setup();
    mf_log_reset();
    [win sendEvent:mouse(NSEventTypeLeftMouseDown, NSMakePoint(30, 30), 1)];
    [win sendEvent:mouse(NSEventTypeLeftMouseUp, NSMakePoint(30, 30), 1)];
    mf_spin(0.1);
    MF_CHECK("click_inner", mf_log_str());
    MF_CHECK("click_made_first_responder", win.firstResponder == inner ? @"inner" : mf_cls(win.firstResponder));
    mf_log_reset();
    [win sendEvent:mouse(NSEventTypeLeftMouseDown, NSMakePoint(150, 100), 2)];
    [win sendEvent:mouse(NSEventTypeLeftMouseUp, NSMakePoint(150, 100), 2)];
    mf_spin(0.1);
    MF_CHECK("double_click_outer", mf_log_str());
    mf_log_reset();
    [win sendEvent:mouse(NSEventTypeLeftMouseDown, NSMakePoint(250, 40), 1)];
    [win sendEvent:mouse(NSEventTypeLeftMouseUp, NSMakePoint(250, 40), 1)];
    mf_spin(0.1);
    MF_CHECK("click_refusing_view", [NSString stringWithFormat:@"%@ fr=%@", mf_log_str(),
                                     win.firstResponder == other ? @"other" : @"not-other"]);
    mf_log_reset();
    [win sendEvent:mouse(NSEventTypeRightMouseDown, NSMakePoint(30, 30), 1)];
    mf_spin(0.1);
    MF_CHECK("right_click_inner", mf_log_str());
    [win close];
}

static void g_actions(void)
{
    setup();
    MFAppDelegate *del = [MFAppDelegate new];
    NSApp.delegate = del;
    [win makeFirstResponder:inner];
    mf_log_reset();
    MF_CHECK("send_to_first_responder", [NSString stringWithFormat:@"%@ %@",
                                         mf_b([NSApp sendAction:@selector(customAction:) to:nil from:nil]), mf_log_str()]);
    MF_CHECK("target_for_action", ({
        id t = [NSApp targetForAction:@selector(customAction:)];
        t == inner ? @"inner" : mf_cls(t);
    }));
    [win makeFirstResponder:nil];
    mf_log_reset();
    MF_CHECK("falls_to_app_delegate", [NSString stringWithFormat:@"%@ %@",
                                       mf_b([NSApp sendAction:@selector(customAction:) to:nil from:nil]), mf_log_str()]);
    mf_log_reset();
    MF_CHECK("unhandled_action", [NSString stringWithFormat:@"%@ %@",
                                  mf_b([NSApp sendAction:@selector(noSuchAction:) to:nil from:nil]), mf_log_str()]);
    MF_CHECK("try_to_perform", mf_b([inner tryToPerform:@selector(customAction:) with:nil]));
    [win close];
}

static void g_queue(void)
{
    setup();
    NSEvent *ev = [NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0
                                    timestamp:0 windowNumber:0 context:nil subtype:7 data1:11 data2:22];
    [NSApp postEvent:ev atStart:NO];
    NSEvent *got = [NSApp nextEventMatchingMask:NSEventMaskApplicationDefined
                                      untilDate:[NSDate dateWithTimeIntervalSinceNow:1]
                                         inMode:NSDefaultRunLoopMode dequeue:YES];
    MF_CHECK("posted_event", got ? [NSString stringWithFormat:@"%lu %d %ld %ld", (unsigned long)got.type, got.subtype,
                                    (long)got.data1, (long)got.data2] : @"(none)");
    MF_CHECK("queue_empty_after", mf_b([NSApp nextEventMatchingMask:NSEventMaskApplicationDefined
                                                          untilDate:[NSDate dateWithTimeIntervalSinceNow:0.1]
                                                             inMode:NSDefaultRunLoopMode dequeue:YES] == nil));
    __block int fired = 0;
    [NSTimer scheduledTimerWithTimeInterval:0.05 repeats:NO block:^(NSTimer *t) { fired++; }];
    dispatch_async(dispatch_get_main_queue(), ^{ fired += 10; });
    mf_spin(0.3);
    MF_CHECK("timer_and_main_queue", [NSString stringWithFormat:@"%d", fired]);
    MF_ENV("no_modifiers_held", mf_b(NSEvent.modifierFlags == 0));
    [win close];
}

MF_GROUPS({ "chain", g_chain }, { "keys", g_keys }, { "mouse", g_mouse }, { "actions", g_actions }, { "queue", g_queue });
MF_MAIN("evt")
