/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw ladder rung 2: a small but complete AppKit form, written by TUR in
 * Objective-C, code only. It uses what most real AppKit applications use in
 * their first window: a split view, a table with a data source in a scroll
 * view, a text view, a pop-up button, a checkbox, a slider, Auto Layout
 * anchors, a toolbar, a sheet (NSAlert) and the standard Edit menu with
 * undo.
 *
 * Standard output, for the harness:
 *     macfw-ready                    window on screen and key
 *     macfw-select ROW               table selection changed
 *     macfw-text LEN                 the text view's text changed
 *     macfw-control NAME VALUE       pop-up, checkbox or slider changed
 *     macfw-sheet BEGIN|END CODE     the alert sheet was shown or dismissed
 * MACFW_QUIT_AFTER=seconds quits by itself.
 */
#import <AppKit/AppKit.h>

static void say(NSString *s)
{
    printf("%s\n", s.UTF8String);
    fflush(stdout);
}

@interface R2Delegate : NSObject <NSApplicationDelegate, NSTableViewDataSource, NSTableViewDelegate,
                                  NSTextViewDelegate, NSToolbarDelegate>
@property (strong) NSWindow *window;
@property (strong) NSTableView *table;
@property (strong) NSTextView *text;
@property (strong) NSArray<NSString *> *rows;
@end

@implementation R2Delegate

- (void)buildMenu
{
    NSMenu *main = [NSMenu new];
    NSMenu *appMenu = [NSMenu new];
    [appMenu addItemWithTitle:@"Quit Rung 2" action:@selector(terminate:) keyEquivalent:@"q"];
    [main addItemWithTitle:@"" action:nil keyEquivalent:@""].submenu = appMenu;
    NSMenu *edit = [[NSMenu alloc] initWithTitle:@"Edit"];
    [edit addItemWithTitle:@"Undo" action:@selector(undo:) keyEquivalent:@"z"];
    [edit addItemWithTitle:@"Redo" action:@selector(redo:) keyEquivalent:@"Z"];
    [edit addItem:[NSMenuItem separatorItem]];
    [edit addItemWithTitle:@"Cut" action:@selector(cut:) keyEquivalent:@"x"];
    [edit addItemWithTitle:@"Copy" action:@selector(copy:) keyEquivalent:@"c"];
    [edit addItemWithTitle:@"Paste" action:@selector(paste:) keyEquivalent:@"v"];
    [edit addItemWithTitle:@"Select All" action:@selector(selectAll:) keyEquivalent:@"a"];
    [main addItemWithTitle:@"Edit" action:nil keyEquivalent:@""].submenu = edit;
    NSApp.mainMenu = main;
}

- (NSScrollView *)tablePane
{
    self.rows = @[ @"Alpha", @"Bravo", @"Charlie", @"Delta", @"Echo", @"Foxtrot", @"Golf", @"Hotel" ];
    self.table = [[NSTableView alloc] initWithFrame:NSZeroRect];
    NSTableColumn *col = [[NSTableColumn alloc] initWithIdentifier:@"name"];
    col.title = @"Name";
    col.width = 160;
    [self.table addTableColumn:col];
    self.table.dataSource = self;
    self.table.delegate = self;
    self.table.usesAlternatingRowBackgroundColors = YES;
    NSScrollView *sv = [NSScrollView new];
    sv.documentView = self.table;
    sv.hasVerticalScroller = YES;
    return sv;
}

- (NSView *)formPane
{
    NSPopUpButton *popup = [[NSPopUpButton alloc] initWithFrame:NSZeroRect pullsDown:NO];
    [popup addItemsWithTitles:@[ @"Small", @"Medium", @"Large" ]];
    popup.target = self;
    popup.action = @selector(popupChanged:);
    NSButton *check = [NSButton checkboxWithTitle:@"Enabled" target:self action:@selector(checkChanged:)];
    NSSlider *slider = [NSSlider sliderWithValue:50 minValue:0 maxValue:100 target:self action:@selector(sliderChanged:)];
    NSButton *alert = [NSButton buttonWithTitle:@"Show Sheet…" target:self action:@selector(showSheet:)];

    self.text = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 300, 200)];
    self.text.allowsUndo = YES;
    self.text.richText = NO;
    self.text.delegate = self;
    self.text.string = @"Edit me.";
    self.text.autoresizingMask = NSViewWidthSizable;
    NSScrollView *textScroll = [NSScrollView new];
    textScroll.documentView = self.text;
    textScroll.hasVerticalScroller = YES;
    textScroll.borderType = NSBezelBorder;

    NSStackView *controls = [NSStackView stackViewWithViews:@[ popup, check, slider, alert ]];
    controls.spacing = 10;
    NSStackView *col = [NSStackView stackViewWithViews:@[ controls, textScroll ]];
    col.orientation = NSUserInterfaceLayoutOrientationVertical;
    col.alignment = NSLayoutAttributeLeading;
    col.edgeInsets = NSEdgeInsetsMake(12, 12, 12, 12);
    [textScroll.widthAnchor constraintEqualToAnchor:col.widthAnchor constant:-24].active = YES;
    [textScroll.heightAnchor constraintGreaterThanOrEqualToConstant:150].active = YES;
    [slider.widthAnchor constraintEqualToConstant:120].active = YES;
    return col;
}

- (void)applicationDidFinishLaunching:(NSNotification *)n
{
    [self buildMenu];
    self.window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 760, 440)
                                              styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                                        NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
                                                backing:NSBackingStoreBuffered
                                                  defer:NO];
    self.window.title = @"TUR macfw rung 2";
    NSToolbar *tb = [[NSToolbar alloc] initWithIdentifier:@"org.tur.macfw.rung2"];
    tb.delegate = self;
    tb.displayMode = NSToolbarDisplayModeLabelOnly;
    self.window.toolbar = tb;

    NSSplitView *split = [NSSplitView new];
    split.vertical = YES;
    split.dividerStyle = NSSplitViewDividerStyleThin;
    [split addArrangedSubview:[self tablePane]];
    [split addArrangedSubview:[self formPane]];
    self.window.contentView = split;
    [split setPosition:200 ofDividerAtIndex:0];
    self.window.contentMinSize = NSMakeSize(560, 320);
    [self.window center];
    [self.window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(0.5 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        say([NSString stringWithFormat:@"macfw-ready visible=%d key=%d rows=%ld", self.window.isVisible,
             self.window.isKeyWindow, (long)self.table.numberOfRows]);
    });
    const char *q = getenv("MACFW_QUIT_AFTER");
    if (q) [NSApp performSelector:@selector(terminate:) withObject:nil afterDelay:atof(q)];
}

- (NSInteger)numberOfRowsInTableView:(NSTableView *)tv { return (NSInteger)self.rows.count; }
- (id)tableView:(NSTableView *)tv objectValueForTableColumn:(NSTableColumn *)c row:(NSInteger)r { return self.rows[(NSUInteger)r]; }
- (void)tableViewSelectionDidChange:(NSNotification *)n { say([NSString stringWithFormat:@"macfw-select %ld", (long)self.table.selectedRow]); }
- (void)textDidChange:(NSNotification *)n { say([NSString stringWithFormat:@"macfw-text %lu", (unsigned long)self.text.string.length]); }
- (void)popupChanged:(NSPopUpButton *)p { say([NSString stringWithFormat:@"macfw-control popup %@", p.titleOfSelectedItem]); }
- (void)checkChanged:(NSButton *)b { say([NSString stringWithFormat:@"macfw-control check %ld", (long)b.state]); }
- (void)sliderChanged:(NSSlider *)s { say([NSString stringWithFormat:@"macfw-control slider %.0f", s.doubleValue]); }

- (void)showSheet:(id)sender
{
    NSAlert *a = [NSAlert new];
    a.messageText = @"A sheet";
    a.informativeText = @"Press OK or Cancel.";
    [a addButtonWithTitle:@"OK"];
    [a addButtonWithTitle:@"Cancel"];
    say(@"macfw-sheet BEGIN 0");
    [a beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse r) {
        say([NSString stringWithFormat:@"macfw-sheet END %ld", (long)r]);
    }];
}

- (NSArray<NSToolbarItemIdentifier> *)toolbarAllowedItemIdentifiers:(NSToolbar *)t
{
    return @[ @"add", NSToolbarFlexibleSpaceItemIdentifier ];
}
- (NSArray<NSToolbarItemIdentifier> *)toolbarDefaultItemIdentifiers:(NSToolbar *)t
{
    return @[ NSToolbarFlexibleSpaceItemIdentifier, @"add" ];
}
- (NSToolbarItem *)toolbar:(NSToolbar *)t itemForItemIdentifier:(NSToolbarItemIdentifier)ident willBeInsertedIntoToolbar:(BOOL)f
{
    NSToolbarItem *item = [[NSToolbarItem alloc] initWithItemIdentifier:ident];
    item.label = @"Add Row";
    item.target = self;
    item.action = @selector(addRow:);
    return item;
}
- (void)addRow:(id)sender
{
    self.rows = [self.rows arrayByAddingObject:[NSString stringWithFormat:@"Row %lu", (unsigned long)self.rows.count + 1]];
    [self.table reloadData];
    say([NSString stringWithFormat:@"macfw-control add %ld", (long)self.table.numberOfRows]);
}

- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)a { return YES; }

@end

int main(int argc, char **argv)
{
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        R2Delegate *d = [R2Delegate new];
        app.delegate = d;
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        [app run];
    }
    return 0;
}
