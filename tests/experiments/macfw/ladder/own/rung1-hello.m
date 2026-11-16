/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw ladder rung 1: the smallest real AppKit application, written by TUR.
 * Code only (no nib), Objective-C: an application delegate, a main menu
 * with Quit, one titled window with a label, a text field and a button.
 *
 * It prints what happens on standard output, so the harness can check the
 * interaction without reading pixels:
 *     macfw-ready                  the window is on screen and key
 *     macfw-click N text=...       the button was pressed (N-th time)
 * and quits by itself after MACFW_QUIT_AFTER seconds when that is set.
 */
#import <AppKit/AppKit.h>

@interface R1Delegate : NSObject <NSApplicationDelegate>
@property (strong) NSWindow *window;
@property (strong) NSTextField *field;
@property (strong) NSTextField *status;
@property NSInteger clicks;
@end

@implementation R1Delegate

- (void)buildMenu
{
    NSMenu *main = [NSMenu new];
    NSMenuItem *appItem = [main addItemWithTitle:@"" action:nil keyEquivalent:@""];
    NSMenu *appMenu = [NSMenu new];
    [appMenu addItemWithTitle:@"About Rung 1" action:@selector(orderFrontStandardAboutPanel:) keyEquivalent:@""];
    [appMenu addItem:[NSMenuItem separatorItem]];
    [appMenu addItemWithTitle:@"Quit Rung 1" action:@selector(terminate:) keyEquivalent:@"q"];
    appItem.submenu = appMenu;
    NSMenuItem *editItem = [main addItemWithTitle:@"Edit" action:nil keyEquivalent:@""];
    NSMenu *edit = [[NSMenu alloc] initWithTitle:@"Edit"];
    [edit addItemWithTitle:@"Cut" action:@selector(cut:) keyEquivalent:@"x"];
    [edit addItemWithTitle:@"Copy" action:@selector(copy:) keyEquivalent:@"c"];
    [edit addItemWithTitle:@"Paste" action:@selector(paste:) keyEquivalent:@"v"];
    [edit addItemWithTitle:@"Select All" action:@selector(selectAll:) keyEquivalent:@"a"];
    editItem.submenu = edit;
    NSApp.mainMenu = main;
}

- (void)applicationDidFinishLaunching:(NSNotification *)n
{
    [self buildMenu];
    self.window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 480, 220)
                                              styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                                        NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
                                                backing:NSBackingStoreBuffered
                                                  defer:NO];
    self.window.title = @"TUR macfw rung 1";
    NSTextField *hello = [NSTextField labelWithString:@"Hello from AppKit"];
    hello.font = [NSFont systemFontOfSize:20];
    self.field = [NSTextField textFieldWithString:@""];
    self.field.placeholderString = @"Type here";
    NSButton *button = [NSButton buttonWithTitle:@"Count" target:self action:@selector(count:)];
    button.keyEquivalent = @"\r";
    self.status = [NSTextField labelWithString:@"Not clicked yet"];
    NSStackView *stack = [NSStackView stackViewWithViews:@[ hello, self.field, button, self.status ]];
    stack.orientation = NSUserInterfaceLayoutOrientationVertical;
    stack.alignment = NSLayoutAttributeLeading;
    stack.spacing = 12;
    stack.edgeInsets = NSEdgeInsetsMake(20, 20, 20, 20);
    self.window.contentView = stack;
    [self.field.widthAnchor constraintEqualToConstant:300].active = YES;
    [self.window center];
    [self.window makeKeyAndOrderFront:nil];
    [self.window makeFirstResponder:self.field];
    [NSApp activateIgnoringOtherApps:YES];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(0.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        printf("macfw-ready visible=%d key=%d\n", self.window.isVisible, self.window.isKeyWindow);
        fflush(stdout);
    });
    const char *q = getenv("MACFW_QUIT_AFTER");
    if (q) [NSApp performSelector:@selector(terminate:) withObject:nil afterDelay:atof(q)];
}

- (void)count:(id)sender
{
    self.clicks++;
    self.status.stringValue = [NSString stringWithFormat:@"Clicked %ld times", (long)self.clicks];
    printf("macfw-click %ld text=%s\n", (long)self.clicks, self.field.stringValue.UTF8String);
    fflush(stdout);
}

- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)a { return YES; }

@end

int main(int argc, char **argv)
{
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        R1Delegate *d = [R1Delegate new];
        app.delegate = d;
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        [app run];
    }
    return 0;
}
