/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw probe `ctl`: controls and menus as programs drive them from code:
 * button types and their state machines, target and action, pop-up buttons,
 * text fields and formatters, sliders, segmented controls, menus, key
 * equivalents and menu validation. Natural sizes depend on fonts and are
 * reported as `metric`. Written from Apple's public documentation only.
 */
#import <AppKit/AppKit.h>
#import "mf.h"

@interface MFTarget : NSObject <NSMenuItemValidation>
@property NSInteger hits;
@property (copy) NSString *last;
@end

@implementation MFTarget
- (void)act:(id)sender
{
    self.hits++;
    self.last = NSStringFromClass([sender class]);
    mf_log_add([NSString stringWithFormat:@"act(%@)", self.last]);
}
- (void)disabledAct:(id)sender { mf_log_add(@"disabledAct"); }
- (BOOL)validateMenuItem:(NSMenuItem *)item
{
    mf_log_add([NSString stringWithFormat:@"validate(%@)", item.title]);
    return item.action != @selector(disabledAct:);
}
@end

static NSString *states(NSButton *b, int clicks)
{
    NSMutableArray *s = [NSMutableArray arrayWithObject:[NSString stringWithFormat:@"%ld", (long)b.state]];
    for (int i = 0; i < clicks; i++) {
        [b performClick:nil];
        [s addObject:[NSString stringWithFormat:@"%ld", (long)b.state]];
    }
    return [s componentsJoinedByString:@">"];
}

static void g_button(void)
{
    [NSApplication sharedApplication];
    MFTarget *t = [MFTarget new];
    NSButton *push = [NSButton buttonWithTitle:@"Go" target:t action:@selector(act:)];
    MF_CHECK("push.defaults", [NSString stringWithFormat:@"%@ bezel=%lu enabled=%@ state=%ld",
                               push.title, (unsigned long)push.bezelStyle, mf_b(push.isEnabled), (long)push.state]);
    MF_METRIC("push.intrinsic", mf_sz(push.intrinsicContentSize));
    mf_log_reset();
    [push performClick:nil];
    MF_CHECK("push.click", [NSString stringWithFormat:@"%ld %@", (long)t.hits, mf_log_str()]);
    push.enabled = NO;
    [push performClick:nil];
    MF_CHECK("push.disabled_click_hits", [NSString stringWithFormat:@"%ld", (long)t.hits]);

    NSButton *check = [NSButton checkboxWithTitle:@"Check" target:nil action:nil];
    MF_CHECK("check.cycle", states(check, 3));
    check.allowsMixedState = YES;
    check.state = NSControlStateValueOff;
    MF_CHECK("check.mixed_cycle", states(check, 4));
    NSButton *radio = [NSButton radioButtonWithTitle:@"R" target:nil action:nil];
    MF_CHECK("radio.cycle", states(radio, 2));
    NSButton *onoff = [NSButton buttonWithTitle:@"T" target:nil action:nil];
    [onoff setButtonType:NSButtonTypePushOnPushOff];
    MF_CHECK("onoff.cycle", states(onoff, 2));
    MF_CHECK("check.state_value_int", ({ check.state = NSControlStateValueOn; [NSString stringWithFormat:@"%d", check.intValue]; }));
    MF_CHECK("key_equivalent_default", [NSString stringWithFormat:@"[%@] %lu", push.keyEquivalent,
                                        (unsigned long)push.keyEquivalentModifierMask]);
    MF_CHECK("radio_group", ({
        /* Radio buttons with the same superview and action act as a group. */
        NSView *box = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 200, 100)];
        NSButton *r1 = [NSButton radioButtonWithTitle:@"1" target:t action:@selector(act:)];
        NSButton *r2 = [NSButton radioButtonWithTitle:@"2" target:t action:@selector(act:)];
        [box addSubview:r1];
        [box addSubview:r2];
        [r1 performClick:nil];
        [r2 performClick:nil];
        [NSString stringWithFormat:@"%ld,%ld", (long)r1.state, (long)r2.state];
    }));
}

static void g_popup(void)
{
    NSPopUpButton *p = [[NSPopUpButton alloc] initWithFrame:NSMakeRect(0, 0, 120, 26) pullsDown:NO];
    [p addItemsWithTitles:@[ @"One", @"Two", @"Two", @"Three" ]];
    MF_CHECK("count_after_duplicates", [NSString stringWithFormat:@"%ld", (long)p.numberOfItems]);
    MF_CHECK("titles", [p.itemTitles componentsJoinedByString:@"|"]);
    MF_CHECK("selected_default", [NSString stringWithFormat:@"%ld %@", (long)p.indexOfSelectedItem, p.titleOfSelectedItem]);
    [p selectItemWithTitle:@"Three"];
    MF_CHECK("select_title", [NSString stringWithFormat:@"%ld", (long)p.indexOfSelectedItem]);
    [p selectItemAtIndex:99];
    MF_CHECK("select_out_of_range", [NSString stringWithFormat:@"%ld", (long)p.indexOfSelectedItem]);
    [p insertItemWithTitle:@"Zero" atIndex:0];
    MF_CHECK("after_insert", [NSString stringWithFormat:@"%ld %@", (long)p.indexOfSelectedItem, p.titleOfSelectedItem]);
    [p removeAllItems];
    MF_CHECK("empty", [NSString stringWithFormat:@"%ld %@", (long)p.indexOfSelectedItem, p.titleOfSelectedItem]);
    NSPopUpButton *pd = [[NSPopUpButton alloc] initWithFrame:NSMakeRect(0, 0, 120, 26) pullsDown:YES];
    [pd addItemsWithTitles:@[ @"Title", @"A", @"B" ]];
    MF_CHECK("pulldown_title", pd.title);
    MF_CHECK("pulldown_selected", [NSString stringWithFormat:@"%ld", (long)pd.indexOfSelectedItem]);
}

static void g_textfield(void)
{
    NSTextField *label = [NSTextField labelWithString:@"Label"];
    NSTextField *field = [NSTextField textFieldWithString:@"12.5abc"];
    MF_CHECK("label.flags", [NSString stringWithFormat:@"editable=%@ selectable=%@ bezeled=%@ drawsBg=%@",
                             mf_b(label.isEditable), mf_b(label.isSelectable), mf_b(label.isBezeled),
                             mf_b(label.drawsBackground)]);
    MF_CHECK("field.flags", [NSString stringWithFormat:@"editable=%@ selectable=%@ bezeled=%@",
                             mf_b(field.isEditable), mf_b(field.isSelectable), mf_b(field.isBezeled)]);
    MF_CHECK("field.values", [NSString stringWithFormat:@"%d %.2f %ld", field.intValue, field.doubleValue,
                              (long)field.integerValue]);
    field.integerValue = 42;
    MF_CHECK("set_integer", field.stringValue);
    field.doubleValue = 0.1;
    MF_CHECK("set_double", field.stringValue);
    NSNumberFormatter *nf = [NSNumberFormatter new];
    nf.locale = [NSLocale localeWithLocaleIdentifier:@"en_US"];
    nf.numberStyle = NSNumberFormatterDecimalStyle;
    field.formatter = nf;
    field.objectValue = @1234.5;
    MF_CHECK("formatted", field.stringValue);
    MF_CHECK("wrapping_label", ({
        NSTextField *w = [NSTextField wrappingLabelWithString:@"x"];
        [NSString stringWithFormat:@"%@ %ld", mf_b(w.isSelectable), (long)w.cell.lineBreakMode];
    }));
    MF_METRIC("label.intrinsic", mf_sz(label.intrinsicContentSize));
    MF_CHECK("placeholder", ({ field.placeholderString = @"hint"; field.placeholderString; }));
    NSSecureTextField *sec = [NSSecureTextField new];
    sec.stringValue = @"pw";
    MF_CHECK("secure_value", sec.stringValue);
}

static void g_misc_controls(void)
{
    NSSlider *s = [NSSlider sliderWithValue:5 minValue:0 maxValue:10 target:nil action:nil];
    s.numberOfTickMarks = 5;
    s.allowsTickMarkValuesOnly = YES;
    MF_CHECK("slider.closest_tick", mf_f([s closestTickMarkValueToValue:6.4]));
    s.doubleValue = 12;
    MF_CHECK("slider.clamped", mf_f(s.doubleValue));
    NSStepper *st = [NSStepper new];
    st.minValue = 0;
    st.maxValue = 3;
    st.increment = 1;
    st.valueWraps = YES;
    st.doubleValue = 3;
    MF_CHECK("stepper.value", mf_f(st.doubleValue));
    NSSegmentedControl *seg = [NSSegmentedControl segmentedControlWithLabels:@[ @"a", @"b", @"c" ]
                                                                trackingMode:NSSegmentSwitchTrackingSelectOne
                                                                      target:nil action:nil];
    MF_CHECK("segmented.count_selected", [NSString stringWithFormat:@"%ld %ld", (long)seg.segmentCount, (long)seg.selectedSegment]);
    seg.selectedSegment = 2;
    MF_CHECK("segmented.select", [NSString stringWithFormat:@"%ld %@", (long)seg.selectedSegment, mf_b([seg isSelectedForSegment:1])]);
    NSProgressIndicator *pi = [NSProgressIndicator new];
    pi.indeterminate = NO;
    pi.minValue = 0;
    pi.maxValue = 50;
    [pi incrementBy:20];
    MF_CHECK("progress", mf_f(pi.doubleValue));
    NSColorWell *cw = [NSColorWell new];
    MF_CHECK("colorwell_class", mf_cls(cw.color));
    NSDatePicker *dp = [NSDatePicker new];
    dp.timeZone = [NSTimeZone timeZoneForSecondsFromGMT:0];
    dp.minDate = [NSDate dateWithTimeIntervalSince1970:0];
    dp.dateValue = [NSDate dateWithTimeIntervalSince1970:-100];
    MF_CHECK("datepicker.clamp", mf_f(dp.dateValue.timeIntervalSince1970));
}

static NSEvent *key_event(NSString *chars, NSEventModifierFlags mods)
{
    return [NSEvent keyEventWithType:NSEventTypeKeyDown location:NSZeroPoint modifierFlags:mods
                           timestamp:0 windowNumber:0 context:nil characters:chars
         charactersIgnoringModifiers:chars isARepeat:NO keyCode:0];
}

static void g_menu(void)
{
    [NSApplication sharedApplication];
    MFTarget *t = [MFTarget new];
    NSMenu *m = [[NSMenu alloc] initWithTitle:@"File"];
    NSMenuItem *open = [m addItemWithTitle:@"Open" action:@selector(act:) keyEquivalent:@"o"];
    open.target = t;
    NSMenuItem *dis = [m addItemWithTitle:@"Never" action:@selector(disabledAct:) keyEquivalent:@"n"];
    dis.target = t;
    [m addItem:[NSMenuItem separatorItem]];
    NSMenuItem *noTarget = [m addItemWithTitle:@"Orphan" action:@selector(nonexistentAction:) keyEquivalent:@""];
    NSMenuItem *shifted = [m addItemWithTitle:@"Shifted" action:@selector(act:) keyEquivalent:@"O"];
    shifted.target = t;
    MF_CHECK("count", [NSString stringWithFormat:@"%ld", (long)m.numberOfItems]);
    MF_CHECK("index_of_title", [NSString stringWithFormat:@"%ld %ld", (long)[m indexOfItemWithTitle:@"Never"],
                                (long)[m indexOfItemWithTitle:@"missing"]]);
    MF_CHECK("default_modifier_mask", [NSString stringWithFormat:@"%lu", (unsigned long)open.keyEquivalentModifierMask]);
    MF_CHECK("separator", mf_b([m itemAtIndex:2].isSeparatorItem));
    MF_CHECK("autoenables", mf_b(m.autoenablesItems));
    mf_log_reset();
    [m update];
    MF_CHECK("update_sequence", mf_log_str());
    MF_CHECK("enabled_after_update", [NSString stringWithFormat:@"%@ %@ %@", mf_b(open.isEnabled), mf_b(dis.isEnabled),
                                      mf_b(noTarget.isEnabled)]);
    mf_log_reset();
    BOOL handled = [m performKeyEquivalent:key_event(@"o", NSEventModifierFlagCommand)];
    MF_CHECK("key_cmd_o", [NSString stringWithFormat:@"%@ %@", mf_b(handled), mf_log_str()]);
    mf_log_reset();
    handled = [m performKeyEquivalent:key_event(@"n", NSEventModifierFlagCommand)];
    MF_CHECK("key_disabled", [NSString stringWithFormat:@"%@ %@", mf_b(handled), mf_log_str()]);
    mf_log_reset();
    handled = [m performKeyEquivalent:key_event(@"O", NSEventModifierFlagCommand | NSEventModifierFlagShift)];
    MF_CHECK("key_cmd_shift_o", [NSString stringWithFormat:@"%@ %@", mf_b(handled), mf_log_str()]);
    mf_log_reset();
    handled = [m performKeyEquivalent:key_event(@"o", NSEventModifierFlagControl)];
    MF_CHECK("key_ctrl_o", [NSString stringWithFormat:@"%@ %@", mf_b(handled), mf_log_str()]);
    NSMenu *main = [NSMenu new];
    NSMenuItem *top = [main addItemWithTitle:@"" action:nil keyEquivalent:@""];
    top.submenu = m;
    NSApp.mainMenu = main;
    MF_CHECK("main_menu_set", mf_b(NSApp.mainMenu == main));
    MF_CHECK("supermenu", mf_b(m.supermenu == main));
    mf_log_reset();
    [m performActionForItemAtIndex:0];
    MF_CHECK("perform_action_index", mf_log_str());
}

MF_GROUPS({ "button", g_button }, { "popup", g_popup }, { "textfield", g_textfield },
          { "misc", g_misc_controls }, { "menu", g_menu });
MF_MAIN("ctl")
