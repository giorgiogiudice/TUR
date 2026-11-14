# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Plain Win32 through ctypes, shared by the experiment's scripts
(docs/reports/exp-inwin-1.md): top-level windows, a picture of the screen
written as PNG with rectangles drawn on it, and small helpers. Nothing but
the Python standard library, so that it runs on any Windows runner.
"""

import ctypes
import ctypes.wintypes as wt
import hashlib
import struct
import time
import zlib

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
try:
    dwmapi = ctypes.WinDLL("dwmapi")
except OSError:  # a Windows without the compositor's library
    dwmapi = None

P = ctypes.c_void_p
LRESULT = ctypes.c_ssize_t
WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, P, wt.LPARAM)


def _fn(dll, name, restype, *argtypes):
    function = getattr(dll, name)
    function.restype = restype
    function.argtypes = list(argtypes)
    return function


EnumWindows = _fn(user32, "EnumWindows", wt.BOOL, WNDENUMPROC, wt.LPARAM)
EnumChildWindows = _fn(user32, "EnumChildWindows", wt.BOOL, P, WNDENUMPROC, wt.LPARAM)
IsWindowVisible = _fn(user32, "IsWindowVisible", wt.BOOL, P)
IsWindow = _fn(user32, "IsWindow", wt.BOOL, P)
IsIconic = _fn(user32, "IsIconic", wt.BOOL, P)
IsZoomed = _fn(user32, "IsZoomed", wt.BOOL, P)
IsWindowEnabled = _fn(user32, "IsWindowEnabled", wt.BOOL, P)
GetWindowTextW = _fn(user32, "GetWindowTextW", ctypes.c_int, P, wt.LPWSTR, ctypes.c_int)
GetClassNameW = _fn(user32, "GetClassNameW", ctypes.c_int, P, wt.LPWSTR, ctypes.c_int)
GetWindowRect = _fn(user32, "GetWindowRect", wt.BOOL, P, ctypes.POINTER(wt.RECT))
GetWindowThreadProcessId = _fn(user32, "GetWindowThreadProcessId", wt.DWORD, P, ctypes.POINTER(wt.DWORD))
GetWindowLongPtrW = _fn(user32, "GetWindowLongPtrW", ctypes.c_ssize_t, P, ctypes.c_int)
GetClassLongPtrW = _fn(user32, "GetClassLongPtrW", ctypes.c_size_t, P, ctypes.c_int)
GetWindow = _fn(user32, "GetWindow", P, P, wt.UINT)
GetForegroundWindow = _fn(user32, "GetForegroundWindow", P)
SetForegroundWindow = _fn(user32, "SetForegroundWindow", wt.BOOL, P)
PostMessageW = _fn(user32, "PostMessageW", wt.BOOL, P, wt.UINT, wt.WPARAM, wt.LPARAM)
SendMessageTimeoutW = _fn(user32, "SendMessageTimeoutW", LRESULT, P, wt.UINT, wt.WPARAM, P, wt.UINT, wt.UINT, ctypes.POINTER(ctypes.c_size_t))
SetWindowPos = _fn(user32, "SetWindowPos", wt.BOOL, P, P, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT)
ShowWindow = _fn(user32, "ShowWindow", wt.BOOL, P, ctypes.c_int)
GetDC = _fn(user32, "GetDC", P, P)
ReleaseDC = _fn(user32, "ReleaseDC", ctypes.c_int, P, P)
GetSystemMetrics = _fn(user32, "GetSystemMetrics", ctypes.c_int, ctypes.c_int)
CreateCompatibleDC = _fn(gdi32, "CreateCompatibleDC", P, P)
CreateCompatibleBitmap = _fn(gdi32, "CreateCompatibleBitmap", P, P, ctypes.c_int, ctypes.c_int)
SelectObject = _fn(gdi32, "SelectObject", P, P, P)
BitBlt = _fn(gdi32, "BitBlt", wt.BOOL, P, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, P, ctypes.c_int, ctypes.c_int, wt.DWORD)
GetDIBits = _fn(gdi32, "GetDIBits", ctypes.c_int, P, P, wt.UINT, wt.UINT, P, P, wt.UINT)
DeleteObject = _fn(gdi32, "DeleteObject", wt.BOOL, P)
DeleteDC = _fn(gdi32, "DeleteDC", wt.BOOL, P)
OpenProcess = _fn(kernel32, "OpenProcess", P, wt.DWORD, wt.BOOL, wt.DWORD)
CloseHandle = _fn(kernel32, "CloseHandle", wt.BOOL, P)
QueryFullProcessImageNameW = _fn(kernel32, "QueryFullProcessImageNameW", wt.BOOL, P, wt.DWORD, wt.LPWSTR, ctypes.POINTER(wt.DWORD))

GWL_STYLE, GWL_EXSTYLE, GW_OWNER = -16, -20, 4
WS_EX_TOOLWINDOW, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOPMOST = 0x80, 0x40000, 0x08000000, 0x8
WM_CLOSE, WM_GETTEXT, WM_GETTEXTLENGTH, WM_GETICON = 0x10, 0x0D, 0x0E, 0x7F
SMTO_ABORTIFHUNG = 0x2


def dpi_aware():
    """Ask for real pixels, so that rectangles match the picture."""
    try:
        function = _fn(user32, "SetProcessDpiAwarenessContext", wt.BOOL, P)
        return bool(function(P(-4 & 0xFFFFFFFFFFFFFFFF)))
    except (AttributeError, OSError):
        return False


def text_of(hwnd):
    buffer = ctypes.create_unicode_buffer(512)
    GetWindowTextW(hwnd, buffer, 512)
    return buffer.value


def class_of(hwnd):
    buffer = ctypes.create_unicode_buffer(256)
    GetClassNameW(hwnd, buffer, 256)
    return buffer.value


def rect_of(hwnd):
    rect = wt.RECT()
    GetWindowRect(hwnd, ctypes.byref(rect))
    return [rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top]


def frame_of(hwnd):
    """The frame a person sees (without the invisible resize border), or None."""
    if dwmapi is None:
        return None
    rect = wt.RECT()
    result = dwmapi.DwmGetWindowAttribute(P(hwnd), 9, ctypes.byref(rect), ctypes.sizeof(rect))
    if result != 0:
        return None
    return [rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top]


def cloaked(hwnd):
    if dwmapi is None:
        return 0
    value = wt.DWORD(0)
    dwmapi.DwmGetWindowAttribute(P(hwnd), 14, ctypes.byref(value), 4)
    return value.value


def pid_of(hwnd):
    pid = wt.DWORD(0)
    GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return pid.value


def exe_of(pid):
    name = ""
    handle = OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if handle:
        buffer = ctypes.create_unicode_buffer(1024)
        size = wt.DWORD(1024)
        if QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            name = buffer.value
        CloseHandle(handle)
    return name


def remote_text(hwnd, limit=4096):
    """A control's text by WM_GETTEXT: carried between processes by the system."""
    buffer = ctypes.create_unicode_buffer(limit)
    done = ctypes.c_size_t(0)
    SendMessageTimeoutW(hwnd, WM_GETTEXT, limit, ctypes.cast(buffer, P), SMTO_ABORTIFHUNG, 2000, ctypes.byref(done))
    return buffer.value


def top_windows(visible_only=True):
    """Top-level windows from the top of the z-order down."""
    found = []

    def each(hwnd, _):
        if visible_only and not IsWindowVisible(hwnd):
            return True
        pid = pid_of(hwnd)
        found.append({
            "hwnd": hwnd,
            "title": text_of(hwnd),
            "class": class_of(hwnd),
            "pid": pid,
            "exe": exe_of(pid).rsplit("\\", 1)[-1].lower(),
            "screen": rect_of(hwnd),
            "z": len(found),
        })
        return True

    callback = WNDENUMPROC(each)
    EnumWindows(callback, 0)
    return found


def children(hwnd):
    found = []

    def each(child, _):
        found.append(child)
        return True

    callback = WNDENUMPROC(each)
    EnumChildWindows(hwnd, callback, 0)
    return found


def wait_window(predicate, timeout=60, visible_only=True):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for window in top_windows(visible_only):
            try:
                if predicate(window):
                    return window
            except (KeyError, TypeError):
                continue
        time.sleep(0.4)
    return None


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG), ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD), ("biCompression", wt.DWORD), ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", wt.LONG), ("biYPelsPerMeter", wt.LONG), ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


def screen_box():
    """The whole desktop: x, y, width, height."""
    return [GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79)]


def grab(box=None):
    """A picture of the screen (or of `box`) as (x, y, width, height, BGRA bytearray)."""
    desk = screen_box()
    if box is None:
        box = desk
    x = max(box[0], desk[0])
    y = max(box[1], desk[1])
    width = max(1, min(box[0] + box[2], desk[0] + desk[2]) - x)
    height = max(1, min(box[1] + box[3], desk[1] + desk[3]) - y)
    screen = GetDC(None)
    memory = CreateCompatibleDC(screen)
    bitmap = CreateCompatibleBitmap(screen, width, height)
    old = SelectObject(memory, bitmap)
    BitBlt(memory, 0, 0, width, height, screen, x, y, 0x00CC0020 | 0x40000000)
    SelectObject(memory, old)
    header = BITMAPINFOHEADER()
    header.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    header.biWidth = width
    header.biHeight = -height
    header.biPlanes = 1
    header.biBitCount = 32
    pixels = ctypes.create_string_buffer(width * height * 4)
    GetDIBits(memory, bitmap, 0, height, pixels, ctypes.byref(header), 0)
    DeleteObject(bitmap)
    DeleteDC(memory)
    ReleaseDC(None, screen)
    return x, y, width, height, bytearray(pixels.raw)


def outline(picture, rect, colour, thickness=2):
    """Draw a rectangle's outline (screen coordinates) on a picture from grab()."""
    x0, y0, width, height, data = picture
    left, top = rect[0] - x0, rect[1] - y0
    right, bottom = left + rect[2], top + rect[3]
    pixel = bytes((colour[2], colour[1], colour[0], 255))
    for t in range(thickness):
        for yy in (top + t, bottom - 1 - t):
            if 0 <= yy < height:
                a, b = max(left, 0), min(right, width)
                if b > a:
                    data[(yy * width + a) * 4:(yy * width + b) * 4] = pixel * (b - a)
        for xx in (left + t, right - 1 - t):
            if 0 <= xx < width:
                for yy in range(max(top, 0), min(bottom, height)):
                    offset = (yy * width + xx) * 4
                    data[offset:offset + 4] = pixel


def save_png(path, picture):
    _, _, width, height, data = picture
    rows = bytearray()
    stride = width * 4
    for yy in range(height):
        row = data[yy * stride:(yy + 1) * stride]
        rgb = bytearray(width * 3)
        rgb[0::3] = row[2::4]
        rgb[1::3] = row[1::4]
        rgb[2::3] = row[0::4]
        rows += b"\0" + rgb

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    with open(path, "wb") as out:
        out.write(b"\x89PNG\r\n\x1a\n")
        out.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        out.write(chunk(b"IDAT", zlib.compress(bytes(rows), 6)))
        out.write(chunk(b"IEND", b""))


PrintWindow = _fn(user32, "PrintWindow", wt.BOOL, P, P, wt.UINT)


def print_window(hwnd):
    """A window's own content, asked of the window (PrintWindow with
    PW_RENDERFULLCONTENT), whether or not anything covers it."""
    rect = rect_of(hwnd)
    width, height = max(rect[2], 1), max(rect[3], 1)
    screen = GetDC(None)
    memory = CreateCompatibleDC(screen)
    bitmap = CreateCompatibleBitmap(screen, width, height)
    old = SelectObject(memory, bitmap)
    ok = PrintWindow(hwnd, memory, 2)
    SelectObject(memory, old)
    header = BITMAPINFOHEADER()
    header.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    header.biWidth = width
    header.biHeight = -height
    header.biPlanes = 1
    header.biBitCount = 32
    pixels = ctypes.create_string_buffer(width * height * 4)
    GetDIBits(memory, bitmap, 0, height, pixels, ctypes.byref(header), 0)
    DeleteObject(bitmap)
    DeleteDC(memory)
    ReleaseDC(None, screen)
    return bool(ok), (rect[0], rect[1], width, height, bytearray(pixels.raw))


def crop(picture, margin):
    """The picture without `margin` pixels on every side."""
    x, y, width, height, data = picture
    if width <= 2 * margin or height <= 2 * margin:
        return picture
    out = bytearray()
    for row in range(margin, height - margin):
        out += data[(row * width + margin) * 4:(row * width + width - margin) * 4]
    return x + margin, y + margin, width - 2 * margin, height - 2 * margin, out


def black_share(picture):
    data = picture[4]
    total = black = 0
    for offset in range(0, len(data) - 4, 4 * 11):
        total += 1
        if data[offset] == 0 and data[offset + 1] == 0 and data[offset + 2] == 0:
            black += 1
    return black / max(total, 1)


def picture_hash(picture):
    return hashlib.sha256(bytes(picture[4])).hexdigest()


def differing(a, b):
    """The share of pixels that differ between two pictures of one size."""
    if a[2] != b[2] or a[3] != b[3]:
        return 1.0
    da, db = a[4], b[4]
    step = 4 * 7  # every seventh pixel is enough to say "the picture changed"
    total = changed = 0
    for offset in range(0, len(da) - 4, step):
        total += 1
        if da[offset:offset + 3] != db[offset:offset + 3]:
            changed += 1
    return changed / max(total, 1)
