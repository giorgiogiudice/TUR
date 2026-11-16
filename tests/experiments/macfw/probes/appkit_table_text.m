/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw probe `tbl`: NSTableView, NSOutlineView, NSTextStorage, NSTextView
 * and NSUndoManager: the data-source and delegate calls a table makes, row
 * geometry with explicit row heights and spacing, selection and its
 * notifications, outline expansion, attributed-string editing and undo.
 * Written from Apple's public documentation only.
 */
#import <AppKit/AppKit.h>
#import "mf.h"

@interface MFTableSource : NSObject <NSTableViewDataSource, NSTableViewDelegate>
@property NSInteger rows;
@property NSInteger valueCalls;
@end

@implementation MFTableSource
- (NSInteger)numberOfRowsInTableView:(NSTableView *)tv { return self.rows; }
- (id)tableView:(NSTableView *)tv objectValueForTableColumn:(NSTableColumn *)c row:(NSInteger)r
{
    self.valueCalls++;
    return [NSString stringWithFormat:@"%@-%ld", c.identifier, (long)r];
}
- (BOOL)tableView:(NSTableView *)tv shouldSelectRow:(NSInteger)r
{
    mf_log_add([NSString stringWithFormat:@"shouldSelect(%ld)", (long)r]);
    return r != 3;
}
- (void)tableViewSelectionDidChange:(NSNotification *)n { mf_log_add(@"selectionDidChange"); }
- (void)tableViewSelectionIsChanging:(NSNotification *)n { mf_log_add(@"selectionIsChanging"); }
@end

@interface MFOutlineSource : NSObject <NSOutlineViewDataSource>
@property (strong) NSDictionary<NSString *, NSArray *> *tree;
@end

@implementation MFOutlineSource
- (NSArray *)kids:(id)item { return self.tree[item ?: @"root"] ?: @[]; }
- (NSInteger)outlineView:(NSOutlineView *)ov numberOfChildrenOfItem:(id)item { return [self kids:item].count; }
- (id)outlineView:(NSOutlineView *)ov child:(NSInteger)i ofItem:(id)item { return [self kids:item][i]; }
- (BOOL)outlineView:(NSOutlineView *)ov isItemExpandable:(id)item { return [self kids:item].count > 0; }
- (id)outlineView:(NSOutlineView *)ov objectValueForTableColumn:(NSTableColumn *)c byItem:(id)item { return item; }
@end

static NSTableView *make_table(MFTableSource *src)
{
    NSTableView *tv = [[NSTableView alloc] initWithFrame:NSMakeRect(0, 0, 300, 200)];
    for (NSString *ident in @[ @"a", @"b" ]) {
        NSTableColumn *c = [[NSTableColumn alloc] initWithIdentifier:ident];
        c.width = 100;
        [tv addTableColumn:c];
    }
    tv.dataSource = src;
    tv.delegate = src;
    return tv;
}

static void g_table(void)
{
    [NSApplication sharedApplication];
    MFTableSource *src = [MFTableSource new];
    src.rows = 100;
    NSTableView *tv = make_table(src);
    MF_METRIC("default_row_height", mf_f(tv.rowHeight));
    MF_METRIC("default_spacing", mf_sz(tv.intercellSpacing));
    tv.rowHeight = 20;
    tv.intercellSpacing = NSMakeSize(3, 2);
    [tv reloadData];
    MF_CHECK("rows", [NSString stringWithFormat:@"%ld", (long)tv.numberOfRows]);
    MF_CHECK("columns", [NSString stringWithFormat:@"%ld", (long)tv.numberOfColumns]);
    MF_CHECK("rect_of_row_10", mf_rect([tv rectOfRow:10]));
    MF_CHECK("rect_of_column_1", mf_rect(NSMakeRect([tv rectOfColumn:1].origin.x, 0, [tv rectOfColumn:1].size.width, 0)));
    MF_CHECK("row_at_point", [NSString stringWithFormat:@"%ld", (long)[tv rowAtPoint:NSMakePoint(5, 230)]]);
    MF_CHECK("rows_in_rect", mf_range([tv rowsInRect:NSMakeRect(0, 30, 10, 100)]));
    MF_CHECK("column_with_identifier", [NSString stringWithFormat:@"%ld %ld", (long)[tv columnWithIdentifier:@"b"],
                                        (long)[tv columnWithIdentifier:@"zz"]]);
    MF_CHECK("selected_default", [NSString stringWithFormat:@"%ld %lu", (long)tv.selectedRow,
                                  (unsigned long)tv.selectedRowIndexes.count]);
    mf_log_reset();
    [tv selectRowIndexes:[NSIndexSet indexSetWithIndex:5] byExtendingSelection:NO];
    MF_CHECK("select_sequence", mf_log_str());
    MF_CHECK("selected", [NSString stringWithFormat:@"%ld", (long)tv.selectedRow]);
    tv.allowsMultipleSelection = YES;
    [tv selectRowIndexes:[NSIndexSet indexSetWithIndexesInRange:NSMakeRange(7, 3)] byExtendingSelection:YES];
    MF_CHECK("extended", [NSString stringWithFormat:@"%lu first=%lu last=%lu", (unsigned long)tv.numberOfSelectedRows,
                          (unsigned long)tv.selectedRowIndexes.firstIndex, (unsigned long)tv.selectedRowIndexes.lastIndex]);
    mf_log_reset();
    src.rows = 6;
    [tv reloadData];
    MF_CHECK("reload_shrinks_selection", [NSString stringWithFormat:@"%lu %ld %@", (unsigned long)tv.numberOfSelectedRows,
                                          (long)tv.selectedRow, mf_log_str()]);
    MF_CHECK("view_based_default", mf_b(tv.rowSizeStyle == NSTableViewRowSizeStyleDefault));
    MF_CHECK("sort_descriptors_default", [NSString stringWithFormat:@"%lu", (unsigned long)tv.sortDescriptors.count]);
    [tv moveColumn:0 toColumn:1];
    MF_CHECK("move_column", [[tv.tableColumns valueForKey:@"identifier"] componentsJoinedByString:@","]);
}

static void g_outline(void)
{
    [NSApplication sharedApplication];
    MFOutlineSource *src = [MFOutlineSource new];
    src.tree = @{ @"root" : @[ @"A", @"B" ], @"A" : @[ @"A1", @"A2" ], @"A2" : @[ @"A2x" ], @"B" : @[ @"B1" ] };
    NSOutlineView *ov = [[NSOutlineView alloc] initWithFrame:NSMakeRect(0, 0, 200, 200)];
    NSTableColumn *c = [[NSTableColumn alloc] initWithIdentifier:@"n"];
    [ov addTableColumn:c];
    ov.outlineTableColumn = c;
    ov.dataSource = src;
    [ov reloadData];
    MF_CHECK("rows_collapsed", [NSString stringWithFormat:@"%ld", (long)ov.numberOfRows]);
    [ov expandItem:@"A"];
    MF_CHECK("rows_expand_A", [NSString stringWithFormat:@"%ld", (long)ov.numberOfRows]);
    [ov expandItem:@"B" expandChildren:YES];
    MF_CHECK("rows_expand_B", [NSString stringWithFormat:@"%ld", (long)ov.numberOfRows]);
    [ov expandItem:nil expandChildren:YES];
    NSMutableArray *rows = [NSMutableArray array];
    for (NSInteger r = 0; r < ov.numberOfRows; r++)
        [rows addObject:[NSString stringWithFormat:@"%@:%ld", [ov itemAtRow:r], (long)[ov levelForRow:r]]];
    MF_CHECK("all_expanded", [rows componentsJoinedByString:@" "]);
    MF_CHECK("parent_of_A2x", [ov parentForItem:@"A2x"]);
    MF_CHECK("row_for_missing", [NSString stringWithFormat:@"%ld", (long)[ov rowForItem:@"zz"]]);
    [ov collapseItem:@"A" collapseChildren:NO];
    MF_CHECK("collapse_A", [NSString stringWithFormat:@"%ld %@", (long)ov.numberOfRows, mf_b([ov isItemExpanded:@"A2"])]);
    MF_METRIC("indentation", mf_f(ov.indentationPerLevel));
}

@interface MFStorageDelegate : NSObject <NSTextStorageDelegate>
@end
@implementation MFStorageDelegate
- (void)textStorage:(NSTextStorage *)ts didProcessEditing:(NSTextStorageEditActions)a range:(NSRange)r changeInLength:(NSInteger)d
{
    mf_log_add([NSString stringWithFormat:@"processed(%lu,%@,%ld)", (unsigned long)a, mf_range(r), (long)d]);
}
@end

static NSString *runs(NSAttributedString *s, NSString *attr)
{
    NSMutableArray *o = [NSMutableArray array];
    [s enumerateAttribute:attr inRange:NSMakeRange(0, s.length) options:0
               usingBlock:^(id v, NSRange r, BOOL *stop) {
                   [o addObject:[NSString stringWithFormat:@"%@=%@", mf_range(r), v ? [v description] : @"-"]];
               }];
    return [o componentsJoinedByString:@" "];
}

static void g_text(void)
{
    [NSApplication sharedApplication];
    MFStorageDelegate *del = [MFStorageDelegate new];
    NSTextStorage *ts = [[NSTextStorage alloc] initWithString:@"hello world"];
    ts.delegate = del;
    mf_log_reset();
    [ts beginEditing];
    [ts replaceCharactersInRange:NSMakeRange(0, 5) withString:@"goodbye"];
    [ts addAttribute:@"tag" value:@"x" range:NSMakeRange(2, 4)];
    [ts endEditing];
    MF_CHECK("edit_sequence", mf_log_str());
    MF_CHECK("string", ts.string);
    MF_CHECK("runs", runs(ts, @"tag"));
    [ts replaceCharactersInRange:NSMakeRange(3, 1) withString:@"OO"];
    MF_CHECK("runs_after_insert_inside", runs(ts, @"tag"));
    MF_CHECK("longest_effective", ({
        NSRange r;
        [ts attribute:@"tag" atIndex:3 longestEffectiveRange:&r inRange:NSMakeRange(0, ts.length)];
        mf_range(r);
    }));

    NSTextView *tv = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 300, 200)];
    MF_CHECK("tv.defaults", [NSString stringWithFormat:@"editable=%@ rich=%@ undo=%@ field_editor=%@",
                             mf_b(tv.isEditable), mf_b(tv.isRichText), mf_b(tv.allowsUndo), mf_b(tv.isFieldEditor)]);
    tv.allowsUndo = YES;
    NSWindow *w = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 300, 200) styleMask:NSWindowStyleMaskTitled
                                                backing:NSBackingStoreBuffered defer:YES];
    w.releasedWhenClosed = NO;
    w.contentView = tv;
    [w makeFirstResponder:tv];
    [tv insertText:@"abc" replacementRange:tv.selectedRange];
    MF_CHECK("insert", [NSString stringWithFormat:@"%@ sel=%@", tv.string, mf_range(tv.selectedRange)]);
    [tv setSelectedRange:NSMakeRange(1, 1)];
    [tv insertText:@"XYZ" replacementRange:tv.selectedRange];
    MF_CHECK("replace_selection", [NSString stringWithFormat:@"%@ sel=%@", tv.string, mf_range(tv.selectedRange)]);
    [tv moveToBeginningOfDocument:nil];
    [tv moveWordForward:nil];
    MF_CHECK("move_word_forward", mf_range(tv.selectedRange));
    [tv selectAll:nil];
    MF_CHECK("select_all", mf_range(tv.selectedRange));
    /* Typing groups close at the end of an event; there is no event here,
     * so whether undo works this way is itself an observation. */
    MF_CHECK("undo", ({ [tv.undoManager undo]; tv.string; }));
    MF_CHECK("can_redo", mf_b(tv.undoManager.canRedo));
    MF_CHECK("redo", ({ [tv.undoManager redo]; tv.string; }));
    tv.string = @"line1\nline2\n";
    MF_CHECK("paragraph_range", mf_range([tv.string paragraphRangeForRange:NSMakeRange(7, 0)]));
    [w close];

    NSUndoManager *um = [NSUndoManager new];
    um.groupsByEvent = NO;
    NSMutableArray *model = [NSMutableArray array];
    [um beginUndoGrouping];
    [model addObject:@"a"];
    [um registerUndoWithTarget:model selector:@selector(removeLastObject) object:nil];
    [um setActionName:@"Add"];
    [um endUndoGrouping];
    MF_CHECK("undo_title", um.undoMenuItemTitle);
    [um undo];
    MF_CHECK("undo_model", [NSString stringWithFormat:@"%lu %@", (unsigned long)model.count, um.redoMenuItemTitle]);
}

MF_GROUPS({ "table", g_table }, { "outline", g_outline }, { "text", g_text });
MF_MAIN("tbl")
