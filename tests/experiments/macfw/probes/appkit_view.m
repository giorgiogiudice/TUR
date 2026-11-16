/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw probe `view`: NSView geometry, the view hierarchy, autoresizing,
 * Auto Layout, NSStackView and drawing into a bitmap. Every value here is
 * independent of fonts and theme: the frames come from fixed sizes and
 * constraints, the pixels from colours the probe fills itself. Written from
 * Apple's public documentation only.
 */
#import <AppKit/AppKit.h>
#import "mf.h"

@interface MFLogView : NSView
@property (copy) NSString *mfName; /* not `tag`: NSView has an integer tag */
@property BOOL flip;
@property (strong) NSColor *fill;
@end

@implementation MFLogView
- (BOOL)isFlipped { return self.flip; }
- (void)viewWillMoveToSuperview:(NSView *)s { mf_log_add([NSString stringWithFormat:@"%@.willMoveToSuperview(%@)", self.mfName, s ? @"view" : @"nil"]); }
- (void)viewDidMoveToSuperview { mf_log_add([NSString stringWithFormat:@"%@.didMoveToSuperview", self.mfName]); }
- (void)viewWillMoveToWindow:(NSWindow *)w { mf_log_add([NSString stringWithFormat:@"%@.willMoveToWindow(%@)", self.mfName, w ? @"window" : @"nil"]); }
- (void)viewDidMoveToWindow { mf_log_add([NSString stringWithFormat:@"%@.didMoveToWindow", self.mfName]); }
- (void)didAddSubview:(NSView *)v { mf_log_add([NSString stringWithFormat:@"%@.didAddSubview", self.mfName]); }
- (void)willRemoveSubview:(NSView *)v { mf_log_add([NSString stringWithFormat:@"%@.willRemoveSubview", self.mfName]); }
- (void)drawRect:(NSRect)r
{
    if (self.fill) {
        [self.fill setFill];
        NSRectFill(self.bounds);
        [[NSColor colorWithSRGBRed:0 green:0 blue:1 alpha:1] setFill];
        NSRectFill(NSMakeRect(0, 0, 10, 10)); /* a marker in the corner the coordinate system calls the origin */
    }
}
@end

static MFLogView *lv(NSString *tag, NSRect frame)
{
    MFLogView *v = [[MFLogView alloc] initWithFrame:frame];
    v.mfName = tag;
    return v;
}

static NSString *name_of(NSView *v)
{
    if (!v) return @"(nil)";
    return [v isKindOfClass:[MFLogView class]] ? ((MFLogView *)v).mfName : NSStringFromClass([v class]);
}

static void g_geometry(void)
{
    NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 400, 300)];
    MFLogView *a = lv(@"a", NSMakeRect(50, 40, 200, 100));
    MFLogView *b = lv(@"b", NSMakeRect(10, 10, 50, 30));
    b.flip = YES;
    [root addSubview:a];
    [a addSubview:b];
    MF_CHECK("default_flipped", mf_b(root.isFlipped));
    MF_CHECK("bounds_default", mf_rect(a.bounds));
    MF_CHECK("convert_b_to_root", mf_pt([b convertPoint:NSMakePoint(5, 5) toView:root]));
    MF_CHECK("convert_root_to_b", mf_pt([b convertPoint:NSMakePoint(70, 60) fromView:root]));
    MF_CHECK("convert_rect_b_to_nil_unattached", mf_rect([b convertRect:NSMakeRect(0, 0, 10, 10) toView:nil]));
    [a setBoundsOrigin:NSMakePoint(20, 10)];
    MF_CHECK("bounds_origin", mf_rect(a.bounds));
    MF_CHECK("convert_after_bounds_origin", mf_pt([b convertPoint:NSZeroPoint toView:root]));
    [a setBoundsSize:NSMakeSize(100, 50)];
    MF_CHECK("bounds_scaled", mf_rect(a.bounds));
    MF_CHECK("convert_after_scale", mf_pt([a convertPoint:NSMakePoint(30, 20) toView:root]));
    MF_CHECK("convert_size_after_scale", mf_sz([a convertSize:NSMakeSize(10, 10) toView:root]));
    [a setBoundsSize:NSMakeSize(200, 100)];
    [a setBoundsOrigin:NSZeroPoint];
    MF_CHECK("hit_b", name_of([root hitTest:NSMakePoint(65, 55)]));
    MF_CHECK("hit_a", name_of([root hitTest:NSMakePoint(240, 130)]));
    MF_CHECK("hit_root", mf_b([root hitTest:NSMakePoint(5, 5)] == root));
    MF_CHECK("hit_outside", mf_cls([root hitTest:NSMakePoint(500, 5)]));
    a.hidden = YES;
    MF_CHECK("hit_hidden", mf_b([root hitTest:NSMakePoint(65, 55)] == root));
    MF_CHECK("b_hidden_or_ancestor", mf_b(b.isHiddenOrHasHiddenAncestor));
    a.hidden = NO;
    a.frameRotation = 90;
    MF_CHECK("rotated_frame", mf_rect(a.frame));
    a.frameRotation = 0;
    MF_CHECK("centerscan", mf_rect([a centerScanRect:NSMakeRect(0.3, 0.6, 10.2, 10.7)]));
}

static void g_hierarchy(void)
{
    mf_log_reset();
    MFLogView *root = lv(@"root", NSMakeRect(0, 0, 300, 200));
    MFLogView *a = lv(@"a", NSMakeRect(0, 0, 10, 10));
    MFLogView *b = lv(@"b", NSMakeRect(0, 0, 10, 10));
    MFLogView *c = lv(@"c", NSMakeRect(0, 0, 10, 10));
    [root addSubview:a];
    MF_CHECK("add_sequence", mf_log_str());
    [root addSubview:b];
    [root addSubview:c positioned:NSWindowBelow relativeTo:b];
    NSMutableArray *order = [NSMutableArray array];
    for (MFLogView *v in root.subviews) [order addObject:v.mfName];
    MF_CHECK("order_below", [order componentsJoinedByString:@","]);
    mf_log_reset();
    [b removeFromSuperview];
    MF_CHECK("remove_sequence", mf_log_str());
    mf_log_reset();
    [a addSubview:c];
    MF_CHECK("reparent_sequence", mf_log_str());
    MF_CHECK("ancestor_shared", name_of([c ancestorSharedWithView:root]));
    MF_CHECK("descendant", mf_b([c isDescendantOf:root]));
    MF_CHECK("opaque_ancestor_class", mf_cls(c.opaqueAncestor));
    mf_log_reset();
    NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 300, 200) styleMask:NSWindowStyleMaskBorderless
                                                backing:NSBackingStoreBuffered defer:YES];
    w.releasedWhenClosed = NO;
    [w.contentView addSubview:root];
    MF_CHECK("into_window_sequence", mf_log_str());
    MF_CHECK("window_set", mf_b(c.window == w));
    mf_log_reset();
    [root removeFromSuperview];
    MF_CHECK("out_of_window_sequence", mf_log_str());
}

static void g_autoresize(void)
{
    NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 400, 300)];
    struct { const char *name; NSAutoresizingMaskOptions m; } cases[] = {
        { "none", NSViewNotSizable },
        { "width", NSViewWidthSizable },
        { "height_minY", NSViewHeightSizable | NSViewMinYMargin },
        { "all_margins", NSViewMinXMargin | NSViewMaxXMargin | NSViewMinYMargin | NSViewMaxYMargin },
        { "width_both_margins", NSViewWidthSizable | NSViewMinXMargin | NSViewMaxXMargin },
        { "everything", NSViewWidthSizable | NSViewHeightSizable | NSViewMinXMargin | NSViewMaxXMargin |
                        NSViewMinYMargin | NSViewMaxYMargin },
    };
    NSMutableArray *vs = [NSMutableArray array];
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        NSView *v = [[NSView alloc] initWithFrame:NSMakeRect(40, 30, 200, 100)];
        v.autoresizingMask = cases[i].m;
        [root addSubview:v];
        [vs addObject:v];
    }
    [root setFrameSize:NSMakeSize(520, 360)];
    for (size_t i = 0; i < vs.count; i++) MF_CHECK(cases[i].name, mf_rect([vs[i] frame]));
    [root setFrameSize:NSMakeSize(100, 80)];
    for (size_t i = 0; i < vs.count; i++) {
        char n[64];
        snprintf(n, sizeof n, "%s.shrunk", cases[i].name);
        MF_CHECK(n, mf_rect([vs[i] frame]));
    }
}

static NSView *sized(CGFloat w, CGFloat h, NSLayoutPriority widthPriority)
{
    NSView *v = [[NSView alloc] initWithFrame:NSZeroRect];
    v.translatesAutoresizingMaskIntoConstraints = NO;
    NSLayoutConstraint *cw = [v.widthAnchor constraintEqualToConstant:w];
    cw.priority = widthPriority; /* set before activation: changing it later between required and not raises */
    cw.active = YES;
    [v.heightAnchor constraintEqualToConstant:h].active = YES;
    return v;
}

static NSView *fixed(CGFloat w, CGFloat h) { return sized(w, h, NSLayoutPriorityRequired); }

static void g_layout(void)
{
    NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 400, 300)];
    NSView *a = fixed(100, 40), *b = [[NSView alloc] initWithFrame:NSZeroRect];
    b.translatesAutoresizingMaskIntoConstraints = NO;
    [root addSubview:a];
    [root addSubview:b];
    [NSLayoutConstraint activateConstraints:@[
        [a.leadingAnchor constraintEqualToAnchor:root.leadingAnchor constant:20],
        [a.topAnchor constraintEqualToAnchor:root.topAnchor constant:10],
        [b.leadingAnchor constraintEqualToAnchor:a.trailingAnchor constant:8],
        [b.trailingAnchor constraintEqualToAnchor:root.trailingAnchor constant:-20],
        [b.topAnchor constraintEqualToAnchor:a.bottomAnchor constant:5],
        [b.bottomAnchor constraintEqualToAnchor:root.bottomAnchor constant:-10],
    ]];
    [root layoutSubtreeIfNeeded];
    MF_CHECK("a", mf_rect(a.frame));
    MF_CHECK("b", mf_rect(b.frame));
    [root setFrameSize:NSMakeSize(600, 200)];
    [root layoutSubtreeIfNeeded];
    MF_CHECK("b_after_resize", mf_rect(b.frame));

    NSView *c = fixed(50, 50);
    [root addSubview:c];
    NSLayoutConstraint *weak = [c.centerXAnchor constraintEqualToAnchor:root.centerXAnchor];
    weak.priority = NSLayoutPriorityDefaultLow;
    NSLayoutConstraint *strong = [c.leadingAnchor constraintGreaterThanOrEqualToAnchor:root.leadingAnchor constant:400];
    [NSLayoutConstraint activateConstraints:@[ weak, strong, [c.centerYAnchor constraintEqualToAnchor:root.centerYAnchor] ]];
    [root layoutSubtreeIfNeeded];
    MF_CHECK("priority_wins", mf_rect(c.frame));
    MF_CHECK("ambiguous", mf_b(c.hasAmbiguousLayout));
    MF_CHECK("fitting_size_fixed", mf_sz(fixed(33, 44).fittingSize));
    MF_CHECK("multiplier", ({
        NSView *r2 = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 300, 100)];
        NSView *d = [[NSView alloc] initWithFrame:NSZeroRect];
        d.translatesAutoresizingMaskIntoConstraints = NO;
        [r2 addSubview:d];
        [NSLayoutConstraint activateConstraints:@[
            [d.widthAnchor constraintEqualToAnchor:r2.widthAnchor multiplier:0.25 constant:10],
            [d.heightAnchor constraintEqualToConstant:20],
            [d.trailingAnchor constraintEqualToAnchor:r2.trailingAnchor],
            [d.bottomAnchor constraintEqualToAnchor:r2.bottomAnchor],
        ]];
        [r2 layoutSubtreeIfNeeded];
        mf_rect(d.frame);
    }));
}

static void g_stack(void)
{
    struct { const char *name; NSStackViewDistribution d; } dists[] = {
        { "fill", NSStackViewDistributionFill },
        { "fill_equally", NSStackViewDistributionFillEqually },
        { "equal_spacing", NSStackViewDistributionEqualSpacing },
        { "gravity", NSStackViewDistributionGravityAreas },
    };
    for (size_t i = 0; i < sizeof(dists) / sizeof(dists[0]); i++) {
        NSView *root = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 400, 100)];
        /* For the fill distributions the views must be able to stretch: their
         * widths are then only preferences (249, below the stack's hugging). */
        BOOL stretch = dists[i].d == NSStackViewDistributionFillEqually || dists[i].d == NSStackViewDistributionFill;
        NSLayoutPriority wp = stretch ? 249 : NSLayoutPriorityRequired;
        NSStackView *s = [NSStackView stackViewWithViews:@[ sized(50, 20, wp), sized(80, 30, wp), sized(30, 40, wp) ]];
        s.distribution = dists[i].d;
        s.spacing = 8;
        s.alignment = NSLayoutAttributeCenterY;
        s.translatesAutoresizingMaskIntoConstraints = NO;
        [root addSubview:s];
        [NSLayoutConstraint activateConstraints:@[
            [s.leadingAnchor constraintEqualToAnchor:root.leadingAnchor],
            [s.trailingAnchor constraintEqualToAnchor:root.trailingAnchor],
            [s.topAnchor constraintEqualToAnchor:root.topAnchor],
            [s.bottomAnchor constraintEqualToAnchor:root.bottomAnchor],
        ]];
        [root layoutSubtreeIfNeeded];
        NSMutableArray *fr = [NSMutableArray array];
        for (NSView *v in s.arrangedSubviews) [fr addObject:mf_rect(v.frame)];
        MF_CHECK(dists[i].name, [fr componentsJoinedByString:@" | "]);
    }
    NSStackView *v = [NSStackView stackViewWithViews:@[ fixed(50, 20), fixed(80, 30) ]];
    v.orientation = NSUserInterfaceLayoutOrientationVertical;
    v.spacing = 4;
    v.edgeInsets = NSEdgeInsetsMake(2, 3, 4, 5);
    MF_CHECK("vertical_fitting", mf_sz(v.fittingSize));
    MF_CHECK("default_spacing", mf_f([NSStackView new].spacing));
    MF_CHECK("default_orientation", [NSString stringWithFormat:@"%ld", (long)[NSStackView new].orientation]);
}

static NSString *pixel(NSBitmapImageRep *rep, NSInteger x, NSInteger y)
{
    NSUInteger p[4] = { 0, 0, 0, 0 };
    [rep getPixel:p atX:x y:y];
    return [NSString stringWithFormat:@"%lu,%lu,%lu", (unsigned long)p[0], (unsigned long)p[1], (unsigned long)p[2]];
}

static void g_draw(void)
{
    for (int flipped = 0; flipped < 2; flipped++) {
        MFLogView *v = lv(@"d", NSMakeRect(0, 0, 40, 30));
        v.flip = flipped;
        v.fill = [NSColor colorWithSRGBRed:1 green:0 blue:0 alpha:1];
        NSBitmapImageRep *rep = [v bitmapImageRepForCachingDisplayInRect:v.bounds];
        [v cacheDisplayInRect:v.bounds toBitmapImageRep:rep];
        const char *k = flipped ? "flipped" : "unflipped";
        char n[64];
        snprintf(n, sizeof n, "%s.size", k);
        MF_CHECK(n, [NSString stringWithFormat:@"%ldx%ld", (long)rep.pixelsWide, (long)rep.pixelsHigh]);
        /* Bitmap rows run top to bottom: in an unflipped view the marker at
         * (0,0) is the bottom-left corner, in a flipped one the top-left. */
        snprintf(n, sizeof n, "%s.top_left", k);
        MF_CHECK_K(n, "num:2", pixel(rep, 2 * (NSInteger)rep.pixelsWide / 40, 2 * (NSInteger)rep.pixelsHigh / 30));
        snprintf(n, sizeof n, "%s.bottom_left", k);
        MF_CHECK_K(n, "num:2", pixel(rep, 2 * (NSInteger)rep.pixelsWide / 40, rep.pixelsHigh - 1 - 2 * (NSInteger)rep.pixelsHigh / 30));
        snprintf(n, sizeof n, "%s.center", k);
        MF_CHECK_K(n, "num:2", pixel(rep, rep.pixelsWide / 2, rep.pixelsHigh / 2));
    }
    NSImage *img = [NSImage imageWithSize:NSMakeSize(20, 20) flipped:NO drawingHandler:^BOOL(NSRect r) {
        [[NSColor colorWithSRGBRed:0 green:1 blue:0 alpha:1] setFill];
        NSRectFill(r);
        return YES;
    }];
    NSBitmapImageRep *r2 = [[NSBitmapImageRep alloc] initWithData:[img TIFFRepresentation]];
    MF_CHECK_K("image_handler_pixel", "num:2", pixel(r2, 5, 5));
    MF_CHECK("png_roundtrip_size", ({
        NSData *png = [r2 representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        NSBitmapImageRep *back = [NSBitmapImageRep imageRepWithData:png];
        [NSString stringWithFormat:@"%ldx%ld", (long)back.pixelsWide, (long)back.pixelsHigh];
    }));
    MF_CHECK_K("color_conversion", "num:0.01", ({
        NSColor *c = [[NSColor colorWithSRGBRed:0.2 green:0.4 blue:0.6 alpha:1] colorUsingColorSpace:NSColorSpace.genericRGBColorSpace];
        [NSString stringWithFormat:@"%.3f,%.3f,%.3f", c.redComponent, c.greenComponent, c.blueComponent];
    }));
    MF_CHECK("named_color_class", mf_b([[NSColor.controlTextColor colorUsingColorSpace:NSColorSpace.sRGBColorSpace] isKindOfClass:[NSColor class]]));
}

MF_GROUPS({ "geometry", g_geometry }, { "hierarchy", g_hierarchy }, { "autoresize", g_autoresize },
          { "layout", g_layout }, { "stack", g_stack }, { "draw", g_draw });
MF_MAIN("view")
