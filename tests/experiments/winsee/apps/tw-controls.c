/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tw-controls.exe: a window with one of each standard Windows control, for
 * the experiment docs/reports/exp-winsee-1.md. Its controls, names and
 * number are known, so what a tier of the bridge finds can be counted
 * against the truth (tests/experiments/winsee/harness/winsee.py, CONTROLS).
 *
 * Everything done to it is written, one line each, to the file named by
 * its first argument, so that the harness can tell that an action arrived
 * in the application and not only that the bridge said so.
 *
 *   tw-controls.exe LOGFILE
 */
#ifndef UNICODE
#define UNICODE
#endif
#include <windows.h>
#include <commctrl.h>
#include <shellapi.h>

enum {
    ID_NAME = 101,
    ID_PASSWORD,
    ID_REMEMBER,
    ID_SMALL,
    ID_MEDIUM,
    ID_LARGE,
    ID_COLOUR,
    ID_FRUIT,
    ID_FILES,
    ID_TREE,
    ID_TABS,
    ID_PREVIEW,
    ID_DRAFT,
    ID_HELP,
    ID_CLOSE,
    ID_STATUS,
    ID_TOOLBAR,
    ID_RESULT,
    IDM_NEW = 201,
    IDM_OPEN,
    IDM_EXIT,
    IDM_WRAP,
    IDM_DETAILS,
    IDM_ABOUT,
    IDT_BACK = 301,
    IDT_FORWARD,
    IDT_REFRESH
};

static HANDLE log_file = INVALID_HANDLE_VALUE;
static HWND result_label, status_bar, tool_bar;

static void log_line(const WCHAR *text)
{
    char utf8[1500];
    DWORD written;
    int n = WideCharToMultiByte(CP_UTF8, 0, text, -1, utf8, (int)sizeof utf8 - 2, NULL, NULL);

    if (result_label)
        SetWindowTextW(result_label, text);
    if (log_file == INVALID_HANDLE_VALUE || n <= 0)
        return;
    utf8[n - 1] = '\n';
    WriteFile(log_file, utf8, (DWORD)n, &written, NULL);
    FlushFileBuffers(log_file);
}

static void log_pair(const WCHAR *what, const WCHAR *name)
{
    WCHAR line[700];

    lstrcpynW(line, what, 100);
    lstrcatW(line, L":");
    lstrcpynW(line + lstrlenW(line), name, 500);
    log_line(line);
}

static HWND make(HWND parent, const WCHAR *cls, const WCHAR *text, DWORD style, int x, int y, int w, int h, int id)
{
    HWND control = CreateWindowExW(0, cls, text, WS_CHILD | WS_VISIBLE | style, x, y, w, h, parent,
                                   (HMENU)(INT_PTR)id, GetModuleHandleW(NULL), NULL);

    if (control)
        SendMessageW(control, WM_SETFONT, (WPARAM)GetStockObject(DEFAULT_GUI_FONT), TRUE);
    return control;
}

static void add_tool(HWND bar, int command, const WCHAR *text)
{
    TBBUTTON button;

    ZeroMemory(&button, sizeof button);
    button.iBitmap = I_IMAGENONE;
    button.idCommand = command;
    button.fsState = TBSTATE_ENABLED;
    button.fsStyle = BTNS_BUTTON | BTNS_AUTOSIZE | BTNS_SHOWTEXT;
    button.iString = (INT_PTR)text;
    SendMessageW(bar, TB_ADDBUTTONSW, 1, (LPARAM)&button);
}

static HTREEITEM add_tree(HWND tree, HTREEITEM parent, const WCHAR *text)
{
    TVINSERTSTRUCTW insert;

    ZeroMemory(&insert, sizeof insert);
    insert.hParent = parent;
    insert.hInsertAfter = TVI_LAST;
    insert.item.mask = TVIF_TEXT;
    insert.item.pszText = (WCHAR *)text;
    return (HTREEITEM)SendMessageW(tree, TVM_INSERTITEMW, 0, (LPARAM)&insert);
}

static void add_row(HWND list, int row, const WCHAR *file, const WCHAR *size)
{
    LVITEMW item;

    ZeroMemory(&item, sizeof item);
    item.mask = LVIF_TEXT;
    item.iItem = row;
    item.pszText = (WCHAR *)file;
    SendMessageW(list, LVM_INSERTITEMW, 0, (LPARAM)&item);
    item.iSubItem = 1;
    item.pszText = (WCHAR *)size;
    SendMessageW(list, LVM_SETITEMW, 0, (LPARAM)&item);
}

static void create_controls(HWND window)
{
    HWND control, tree;
    HTREEITEM settings, sound;
    LVCOLUMNW column;
    TCITEMW tab;
    HMENU bar = CreateMenu(), file = CreatePopupMenu(), options = CreatePopupMenu(), help = CreatePopupMenu();

    AppendMenuW(file, MF_STRING, IDM_NEW, L"&New\tCtrl+N");
    AppendMenuW(file, MF_STRING, IDM_OPEN, L"&Open...\tCtrl+O");
    AppendMenuW(file, MF_SEPARATOR, 0, NULL);
    AppendMenuW(file, MF_STRING, IDM_EXIT, L"E&xit");
    AppendMenuW(options, MF_STRING, IDM_WRAP, L"&Word wrap");
    AppendMenuW(options, MF_STRING | MF_CHECKED, IDM_DETAILS, L"Show &details");
    AppendMenuW(help, MF_STRING, IDM_ABOUT, L"&About");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)file, L"&File");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)options, L"&Options");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)help, L"&Help");
    SetMenu(window, bar);

    tool_bar = CreateWindowExW(0, TOOLBARCLASSNAMEW, NULL, WS_CHILD | WS_VISIBLE | TBSTYLE_LIST | TBSTYLE_FLAT, 0, 0,
                               0, 0, window, (HMENU)(INT_PTR)ID_TOOLBAR, GetModuleHandleW(NULL), NULL);
    SendMessageW(tool_bar, TB_BUTTONSTRUCTSIZE, sizeof(TBBUTTON), 0);
    SendMessageW(tool_bar, WM_SETFONT, (WPARAM)GetStockObject(DEFAULT_GUI_FONT), TRUE);
    add_tool(tool_bar, IDT_BACK, L"Back");
    add_tool(tool_bar, IDT_FORWARD, L"Forward");
    add_tool(tool_bar, IDT_REFRESH, L"Refresh");
    SendMessageW(tool_bar, TB_AUTOSIZE, 0, 0);

    make(window, L"Static", L"Name:", 0, 10, 46, 75, 20, 0);
    make(window, L"Edit", L"", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, 90, 44, 180, 22, ID_NAME);
    make(window, L"Static", L"Password:", 0, 10, 74, 75, 20, 0);
    make(window, L"Edit", L"", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL | ES_PASSWORD, 90, 72, 180, 22, ID_PASSWORD);
    make(window, L"Button", L"&Remember me", WS_TABSTOP | BS_AUTOCHECKBOX, 90, 100, 180, 20, ID_REMEMBER);
    make(window, L"Button", L"Size", BS_GROUPBOX, 10, 124, 262, 52, 0);
    make(window, L"Button", L"Small", WS_TABSTOP | WS_GROUP | BS_AUTORADIOBUTTON, 20, 146, 70, 20, ID_SMALL);
    make(window, L"Button", L"Medium", BS_AUTORADIOBUTTON, 95, 146, 80, 20, ID_MEDIUM);
    make(window, L"Button", L"Large", BS_AUTORADIOBUTTON, 180, 146, 70, 20, ID_LARGE);
    SendDlgItemMessageW(window, ID_SMALL, BM_SETCHECK, BST_CHECKED, 0);
    make(window, L"Static", L"Colour:", WS_GROUP, 10, 188, 75, 20, 0);
    control = make(window, L"ComboBox", L"", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST, 90, 185, 180, 200, ID_COLOUR);
    SendMessageW(control, CB_ADDSTRING, 0, (LPARAM)L"Red");
    SendMessageW(control, CB_ADDSTRING, 0, (LPARAM)L"Green");
    SendMessageW(control, CB_ADDSTRING, 0, (LPARAM)L"Blue");
    SendMessageW(control, CB_SETCURSEL, 0, 0);
    make(window, L"Static", L"Fruit:", 0, 10, 218, 75, 20, 0);
    control = make(window, L"ListBox", L"", WS_TABSTOP | WS_BORDER | WS_VSCROLL | LBS_NOTIFY, 90, 216, 180, 74, ID_FRUIT);
    SendMessageW(control, LB_ADDSTRING, 0, (LPARAM)L"Apple");
    SendMessageW(control, LB_ADDSTRING, 0, (LPARAM)L"Banana");
    SendMessageW(control, LB_ADDSTRING, 0, (LPARAM)L"Cherry");
    SendMessageW(control, LB_ADDSTRING, 0, (LPARAM)L"Damson");

    control = make(window, WC_LISTVIEWW, L"", WS_TABSTOP | WS_BORDER | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
                   290, 44, 330, 100, ID_FILES);
    ZeroMemory(&column, sizeof column);
    column.mask = LVCF_TEXT | LVCF_WIDTH;
    column.cx = 200;
    column.pszText = (WCHAR *)L"File";
    SendMessageW(control, LVM_INSERTCOLUMNW, 0, (LPARAM)&column);
    column.cx = 100;
    column.pszText = (WCHAR *)L"Size";
    SendMessageW(control, LVM_INSERTCOLUMNW, 1, (LPARAM)&column);
    add_row(control, 0, L"report.txt", L"12 KB");
    add_row(control, 1, L"photo.png", L"2 MB");
    add_row(control, 2, L"notes.md", L"1 KB");

    tree = make(window, WC_TREEVIEWW, L"",
                WS_TABSTOP | WS_BORDER | TVS_HASLINES | TVS_HASBUTTONS | TVS_LINESATROOT | TVS_SHOWSELALWAYS, 290, 152,
                160, 138, ID_TREE);
    settings = add_tree(tree, TVI_ROOT, L"Settings");
    add_tree(tree, settings, L"Display");
    sound = add_tree(tree, settings, L"Sound");
    add_tree(tree, sound, L"Volume");
    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND, (LPARAM)settings);

    control = make(window, WC_TABCONTROLW, L"", WS_TABSTOP, 460, 152, 160, 138, ID_TABS);
    ZeroMemory(&tab, sizeof tab);
    tab.mask = TCIF_TEXT;
    tab.pszText = (WCHAR *)L"General";
    SendMessageW(control, TCM_INSERTITEMW, 0, (LPARAM)&tab);
    tab.pszText = (WCHAR *)L"Advanced";
    SendMessageW(control, TCM_INSERTITEMW, 1, (LPARAM)&tab);

    make(window, L"Button", L"&Preview", WS_TABSTOP | BS_PUSHBUTTON, 10, 302, 90, 26, ID_PREVIEW);
    make(window, L"Button", L"Save draft", WS_TABSTOP | BS_PUSHBUTTON, 110, 302, 90, 26, ID_DRAFT);
    make(window, L"Button", L"Help", WS_TABSTOP | BS_PUSHBUTTON, 210, 302, 90, 26, ID_HELP);
    make(window, L"Button", L"Close", WS_TABSTOP | BS_PUSHBUTTON, 310, 302, 90, 26, ID_CLOSE);
    result_label = make(window, L"Static", L"Nothing done yet", 0, 10, 338, 610, 20, ID_RESULT);

    status_bar = CreateWindowExW(0, STATUSCLASSNAMEW, L"Ready", WS_CHILD | WS_VISIBLE, 0, 0, 0, 0, window,
                                 (HMENU)(INT_PTR)ID_STATUS, GetModuleHandleW(NULL), NULL);
}

static void command(HWND window, int id, int code, HWND control)
{
    WCHAR text[300];

    text[0] = 0;
    switch (id) {
    case ID_PREVIEW:
    case ID_DRAFT:
    case ID_HELP:
    case ID_CLOSE:
        if (control && code == BN_CLICKED) {
            GetWindowTextW(control, text, 300);
            log_pair(L"clicked", text);
        }
        break;
    case ID_REMEMBER:
        if (control && code == BN_CLICKED)
            log_pair(L"check:Remember me", SendMessageW(control, BM_GETCHECK, 0, 0) == BST_CHECKED ? L"1" : L"0");
        break;
    case ID_SMALL:
    case ID_MEDIUM:
    case ID_LARGE:
        if (control && code == BN_CLICKED && SendMessageW(control, BM_GETCHECK, 0, 0) == BST_CHECKED) {
            GetWindowTextW(control, text, 300);
            log_pair(L"radio", text);
        }
        break;
    case ID_COLOUR:
        if (control && code == CBN_SELCHANGE) {
            GetWindowTextW(control, text, 300);
            log_pair(L"combo", text);
        }
        break;
    case ID_FRUIT:
        if (control && code == LBN_SELCHANGE) {
            LRESULT index = SendMessageW(control, LB_GETCURSEL, 0, 0);

            if (index != LB_ERR && SendMessageW(control, LB_GETTEXTLEN, (WPARAM)index, 0) < 290)
                SendMessageW(control, LB_GETTEXT, (WPARAM)index, (LPARAM)text);
            log_pair(L"list", text);
        }
        break;
    case ID_NAME:
        if (control && code == EN_CHANGE) {
            GetWindowTextW(control, text, 300);
            log_pair(L"text:Name", text);
        }
        break;
    case IDM_NEW:
        log_line(L"menu:New");
        break;
    case IDM_OPEN:
        log_line(L"menu:Open");
        break;
    case IDM_ABOUT:
        log_line(L"menu:About");
        break;
    case IDM_WRAP:
    case IDM_DETAILS: {
        HMENU menu = GetMenu(window);
        UINT state = GetMenuState(menu, (UINT)id, MF_BYCOMMAND);
        int checked = !(state & MF_CHECKED);

        CheckMenuItem(menu, (UINT)id, MF_BYCOMMAND | (checked ? MF_CHECKED : MF_UNCHECKED));
        log_pair(id == IDM_WRAP ? L"menu:Word wrap" : L"menu:Show details", checked ? L"1" : L"0");
        break;
    }
    case IDM_EXIT:
        log_line(L"menu:Exit");
        DestroyWindow(window);
        break;
    case IDT_BACK:
        log_line(L"toolbar:Back");
        break;
    case IDT_FORWARD:
        log_line(L"toolbar:Forward");
        break;
    case IDT_REFRESH:
        log_line(L"toolbar:Refresh");
        break;
    default:
        break;
    }
}

static void notify(HWND window, const NMHDR *header)
{
    WCHAR text[200];

    (void)window;
    if (header->idFrom == ID_TABS && header->code == (UINT)TCN_SELCHANGE) {
        LRESULT index = SendMessageW(header->hwndFrom, TCM_GETCURSEL, 0, 0);

        log_pair(L"tab", index == 1 ? L"Advanced" : L"General");
    } else if (header->idFrom == ID_TREE && header->code == (UINT)TVN_SELCHANGEDW) {
        const NMTREEVIEWW *change = (const NMTREEVIEWW *)header;
        TVITEMW item;

        ZeroMemory(&item, sizeof item);
        item.mask = TVIF_TEXT | TVIF_HANDLE;
        item.hItem = change->itemNew.hItem;
        item.pszText = text;
        item.cchTextMax = 199;
        text[0] = 0;
        SendMessageW(header->hwndFrom, TVM_GETITEMW, 0, (LPARAM)&item);
        log_pair(L"tree", text);
    } else if (header->idFrom == ID_FILES && header->code == (UINT)LVN_ITEMCHANGED) {
        const NMLISTVIEW *change = (const NMLISTVIEW *)header;

        if ((change->uNewState & LVIS_SELECTED) && !(change->uOldState & LVIS_SELECTED)) {
            LVITEMW item;

            ZeroMemory(&item, sizeof item);
            item.iSubItem = 0;
            item.pszText = text;
            item.cchTextMax = 199;
            text[0] = 0;
            SendMessageW(header->hwndFrom, LVM_GETITEMTEXTW, (WPARAM)change->iItem, (LPARAM)&item);
            log_pair(L"row", text);
        }
    }
}

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    switch (message) {
    case WM_CREATE:
        create_controls(window);
        return 0;
    case WM_COMMAND:
        command(window, LOWORD(wparam), HIWORD(wparam), (HWND)lparam);
        return 0;
    case WM_NOTIFY:
        notify(window, (const NMHDR *)lparam);
        return 0;
    case WM_SIZE:
        if (status_bar)
            SendMessageW(status_bar, WM_SIZE, 0, 0);
        if (tool_bar)
            SendMessageW(tool_bar, TB_AUTOSIZE, 0, 0);
        return 0;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    default:
        return DefWindowProcW(window, message, wparam, lparam);
    }
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    INITCOMMONCONTROLSEX controls;
    WNDCLASSW cls;
    HWND window;
    MSG message;
    RECT rc;
    int argc = 0;
    WCHAR **argv = CommandLineToArgvW(GetCommandLineW(), &argc);

    (void)previous;
    (void)command_line;
    if (argv && argc > 1)
        log_file = CreateFileW(argv[1], GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, CREATE_ALWAYS,
                               FILE_ATTRIBUTE_NORMAL, NULL);
    controls.dwSize = sizeof controls;
    controls.dwICC = ICC_WIN95_CLASSES | ICC_BAR_CLASSES | ICC_LISTVIEW_CLASSES | ICC_TREEVIEW_CLASSES | ICC_TAB_CLASSES;
    InitCommonControlsEx(&controls);

    ZeroMemory(&cls, sizeof cls);
    cls.lpfnWndProc = window_proc;
    cls.hInstance = instance;
    cls.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    cls.hbrBackground = (HBRUSH)(COLOR_BTNFACE + 1);
    cls.lpszClassName = L"TwControls";
    if (!RegisterClassW(&cls))
        return 2;
    SetRect(&rc, 0, 0, 632, 392);
    AdjustWindowRect(&rc, WS_OVERLAPPEDWINDOW, TRUE);
    window = CreateWindowExW(0, L"TwControls", L"TUR controls test", WS_OVERLAPPEDWINDOW, 60, 60, rc.right - rc.left,
                             rc.bottom - rc.top, NULL, NULL, instance, NULL);
    if (!window)
        return 3;
    ShowWindow(window, show ? show : SW_SHOW);
    UpdateWindow(window);
    log_line(L"started");
    while (GetMessageW(&message, NULL, 0, 0) > 0) {
        if (!IsDialogMessageW(window, &message)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    if (log_file != INVALID_HANDLE_VALUE)
        CloseHandle(log_file);
    return 0;
}
