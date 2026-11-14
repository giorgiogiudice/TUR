// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The probe application of the Android experiment
// (docs/reports/exp-android-1.md). One activity; what it does is chosen by
// the string extra "phase" (several may be given, separated by commas):
//
//   basic    clipboard out and in, a notification, life-cycle and input logging
//   bench    the same integer and floating-point work in Java and in native code
//   open     ask the system for a document to open, and read it
//   save     ask the system for a document to create, and write it
//   view     ask the system to show a web address
//   share    offer a line of text to other applications
//   webview  put a WebView with a line of HTML in the window
//
// Every fact is logged as "key=value" under the tag TURPROBE, printed on
// standard output, shown in the window and appended to files/turprobe.log.
// Each step catches Throwable: under a translation layer a missing method is
// an Error, and the probe must go on to the next step.
package org.tur.probe;

import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.res.Configuration;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.Editable;
import android.text.TextWatcher;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Arrays;

public class ProbeActivity extends Activity {
    static final String TAG = "TURPROBE";
    static final int REQUEST_OPEN = 41;
    static final int REQUEST_SAVE = 42;

    private final StringBuilder all = new StringBuilder();
    private Handler handler;
    private TextView out;
    private LinearLayout root;
    private String phase = "basic";
    private int taps;
    private int backs;
    private int lastWidth = -1;
    private int lastHeight = -1;

    void say(String key, Object value) {
        final String line = key + "=" + value;
        Log.i(TAG, line);
        System.out.println("TURPROBE " + line);
        System.out.flush();
        try {
            FileOutputStream f = new FileOutputStream(new File(getFilesDir(), "turprobe.log"), true);
            f.write((line + "\n").getBytes("UTF-8"));
            f.close();
        } catch (Throwable t) {
            // The log line above is the record; the file is a convenience.
        }
        Runnable show = new Runnable() {
            @Override
            public void run() {
                all.append(line).append('\n');
                if (out != null) {
                    try {
                        out.setText(all.toString());
                    } catch (Throwable t) {
                        Log.i(TAG, "show_failed=" + t);
                    }
                }
            }
        };
        if (Looper.myLooper() == Looper.getMainLooper()) {
            show.run();
        } else if (handler != null) {
            handler.post(show);
        }
    }

    boolean has(String name) {
        return ("," + phase + ",").contains("," + name + ",");
    }

    void later(long ms, final String name, final Runnable step) {
        handler.postDelayed(new Runnable() {
            @Override
            public void run() {
                try {
                    step.run();
                } catch (Throwable t) {
                    say(name + "_error", t);
                }
            }
        }, ms);
    }

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        handler = new Handler(Looper.getMainLooper());
        try {
            String p = getIntent().getStringExtra("phase");
            if (p != null && p.length() > 0) {
                phase = p;
            }
        } catch (Throwable t) {
            Log.i(TAG, "extra_error=" + t);
        }

        try {
            root = new LinearLayout(this);
            root.setOrientation(LinearLayout.VERTICAL);
            root.setBackgroundColor(0xFFFFFFFF);
            root.setPadding(24, 24, 24, 24);

            TextView title = new TextView(this);
            title.setText("TUR probe: " + phase);
            title.setTextSize(22);
            title.setTextColor(0xFF102040);
            title.setContentDescription("tur-probe-title");
            root.addView(title);

            Button tap = new Button(this);
            tap.setText("tur-probe-tap");
            tap.setContentDescription("tur-probe-tap-button");
            tap.setOnClickListener(new View.OnClickListener() {
                @Override
                public void onClick(View v) {
                    taps++;
                    say("tap", taps);
                }
            });
            root.addView(tap);

            EditText edit = new EditText(this);
            edit.setHint("tur-probe-edit");
            edit.addTextChangedListener(new TextWatcher() {
                @Override
                public void beforeTextChanged(CharSequence s, int start, int count, int after) {
                }

                @Override
                public void onTextChanged(CharSequence s, int start, int before, int count) {
                }

                @Override
                public void afterTextChanged(Editable s) {
                    say("text", s.toString());
                }
            });
            root.addView(edit);

            out = new TextView(this);
            out.setTextSize(12);
            out.setTextColor(0xFF000000);
            root.addView(out);

            root.addOnLayoutChangeListener(new View.OnLayoutChangeListener() {
                @Override
                public void onLayoutChange(View v, int l, int t, int r, int b, int ol, int ot, int or, int ob) {
                    int w = r - l;
                    int h = b - t;
                    if (w != lastWidth || h != lastHeight) {
                        lastWidth = w;
                        lastHeight = h;
                        say("layout", w + "x" + h);
                    }
                }
            });
            root.setOnTouchListener(new View.OnTouchListener() {
                @Override
                public boolean onTouch(View v, MotionEvent e) {
                    if (e.getAction() == MotionEvent.ACTION_DOWN) {
                        say("touch", (int) e.getX() + "," + (int) e.getY());
                    }
                    return false;
                }
            });
            setContentView(root);
        } catch (Throwable t) {
            say("ui_error", t);
        }

        say("phase", phase);
        environment();

        if (has("basic")) {
            later(1500, "clipboard_set", new Runnable() {
                @Override
                public void run() {
                    ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
                    cm.setPrimaryClip(ClipData.newPlainText("tur", "tur-probe-clip-from-android"));
                    say("clipboard_set", "tur-probe-clip-from-android");
                }
            });
            later(2500, "notify", new Runnable() {
                @Override
                public void run() {
                    notifyOnce();
                }
            });
            // The harness reads the host clipboard after "clipboard_set" and
            // then writes its own text to it; these two reads come after.
            later(9000, "clipboard_get", new Runnable() {
                @Override
                public void run() {
                    readClipboard();
                }
            });
            later(14000, "clipboard_get", new Runnable() {
                @Override
                public void run() {
                    readClipboard();
                }
            });
        }
        if (has("bench")) {
            later(1000, "bench", new Runnable() {
                @Override
                public void run() {
                    new Thread(new Runnable() {
                        @Override
                        public void run() {
                            bench();
                        }
                    }, "tur-bench").start();
                }
            });
        }
        if (has("open")) {
            later(2000, "open", new Runnable() {
                @Override
                public void run() {
                    Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                    i.addCategory(Intent.CATEGORY_OPENABLE);
                    i.setType("*/*");
                    startActivityForResult(i, REQUEST_OPEN);
                    say("open_started", "ACTION_OPEN_DOCUMENT");
                }
            });
        }
        if (has("save")) {
            later(2000, "save", new Runnable() {
                @Override
                public void run() {
                    Intent i = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                    i.addCategory(Intent.CATEGORY_OPENABLE);
                    i.setType("text/plain");
                    i.putExtra(Intent.EXTRA_TITLE, "tur-probe-saved.txt");
                    startActivityForResult(i, REQUEST_SAVE);
                    say("save_started", "ACTION_CREATE_DOCUMENT");
                }
            });
        }
        if (has("view")) {
            later(2000, "view", new Runnable() {
                @Override
                public void run() {
                    startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse("https://example.org/tur-probe")));
                    say("view_started", "https://example.org/tur-probe");
                }
            });
        }
        if (has("share")) {
            later(2000, "share", new Runnable() {
                @Override
                public void run() {
                    Intent i = new Intent(Intent.ACTION_SEND);
                    i.setType("text/plain");
                    i.putExtra(Intent.EXTRA_TEXT, "tur-probe-shared-text");
                    startActivity(Intent.createChooser(i, "tur-probe-share"));
                    say("share_started", "ACTION_SEND");
                }
            });
        }
        if (has("webview")) {
            later(1500, "webview", new Runnable() {
                @Override
                public void run() {
                    android.webkit.WebView w = new android.webkit.WebView(ProbeActivity.this);
                    w.loadData("<html><body><h1>tur-probe-webview</h1><p>rendered by the WebView</p></body></html>",
                            "text/html", "utf-8");
                    root.addView(w, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 300));
                    say("webview", "created " + w.getClass().getName());
                }
            });
        }
    }

    void environment() {
        try {
            say("sdk_int", Build.VERSION.SDK_INT);
            say("release", Build.VERSION.RELEASE);
            say("device", Build.MANUFACTURER + " / " + Build.MODEL);
            say("supported_abis", Arrays.toString(Build.SUPPORTED_ABIS));
            say("os_arch", System.getProperty("os.arch"));
            say("vm", System.getProperty("java.vm.name") + " " + System.getProperty("java.vm.version"));
            DisplayMetrics dm = getResources().getDisplayMetrics();
            say("display", dm.widthPixels + "x" + dm.heightPixels + " at " + dm.densityDpi + " dpi");
            say("files_dir", getFilesDir());
            say("native_library_dir", getApplicationInfo().nativeLibraryDir);
        } catch (Throwable t) {
            say("environment_error", t);
        }
        try {
            say("external_files_dir", getExternalFilesDir(null));
        } catch (Throwable t) {
            say("external_files_dir_error", t);
        }
        try {
            getPackageManager().getPackageInfo("com.google.android.gms", 0);
            say("google_play_services", "present");
        } catch (Throwable t) {
            say("google_play_services", "absent (" + t.getClass().getSimpleName() + ")");
        }
    }

    void readClipboard() {
        ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData cd = cm.getPrimaryClip();
        String s = "(empty)";
        if (cd != null && cd.getItemCount() > 0) {
            s = String.valueOf(cd.getItemAt(0).coerceToText(this));
        }
        say("clipboard_get", s);
    }

    void notifyOnce() {
        NotificationManager nm = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        Notification.Builder nb;
        if (Build.VERSION.SDK_INT >= 26) {
            NotificationChannel ch = new NotificationChannel("tur", "TUR probe", NotificationManager.IMPORTANCE_DEFAULT);
            nm.createNotificationChannel(ch);
            nb = new Notification.Builder(this, "tur");
        } else {
            nb = new Notification.Builder(this);
        }
        nb.setContentTitle("tur-probe-notification");
        nb.setContentText("from the TUR probe");
        nb.setSmallIcon(android.R.drawable.ic_dialog_info);
        nm.notify(7, nb.build());
        say("notify", "posted");
    }

    // The same work as probe.c, so that the checksums can be compared.
    static long benchInt(int rounds) {
        byte[] buf = new byte[65536];
        long x = 88172645463325252L;
        for (int i = 0; i < buf.length; i++) {
            x ^= x << 13;
            x ^= x >>> 7;
            x ^= x << 17;
            buf[i] = (byte) x;
        }
        long h = 0xcbf29ce484222325L;
        for (int r = 0; r < rounds; r++) {
            for (int i = 0; i < buf.length; i++) {
                h ^= (buf[i] & 0xff);
                h *= 0x100000001b3L;
            }
            buf[r & 0xffff] = (byte) h;
        }
        return h;
    }

    static double benchFp(int rounds) {
        final int n = 96;
        double[] a = new double[n * n];
        double[] b = new double[n * n];
        double[] c = new double[n * n];
        for (int i = 0; i < n * n; i++) {
            a[i] = (i % 7) * 0.5 + 1.0;
            b[i] = (i % 5) * 0.25 + 0.5;
        }
        double s = 0.0;
        for (int r = 0; r < rounds; r++) {
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < n; j++) {
                    double t = 0.0;
                    for (int k = 0; k < n; k++) {
                        t += a[i * n + k] * b[k * n + j];
                    }
                    c[i * n + j] = t;
                }
            }
            s += c[r % (n * n)];
            a[r % (n * n)] += 1.0;
        }
        return s;
    }

    void bench() {
        final int intRounds = 2000;
        final int fpRounds = 200;
        try {
            // Twice: the first pass includes the runtime warming up.
            for (int pass = 1; pass <= 2; pass++) {
                long t0 = System.nanoTime();
                long h = benchInt(intRounds);
                long t1 = System.nanoTime();
                double s = benchFp(fpRounds);
                long t2 = System.nanoTime();
                say("bench_java_int_ms_pass" + pass, (t1 - t0) / 1000000);
                say("bench_java_fp_ms_pass" + pass, (t2 - t1) / 1000000);
                say("bench_java_checksum", Long.toHexString(h) + " " + s);
            }
        } catch (Throwable t) {
            say("bench_java_error", t);
        }
        try {
            System.loadLibrary("turprobe");
            say("native_abi", Native.abi());
            for (int pass = 1; pass <= 2; pass++) {
                long t0 = System.nanoTime();
                long h = Native.benchInt(intRounds);
                long t1 = System.nanoTime();
                double s = Native.benchFp(fpRounds);
                long t2 = System.nanoTime();
                say("bench_native_int_ms_pass" + pass, (t1 - t0) / 1000000);
                say("bench_native_fp_ms_pass" + pass, (t2 - t1) / 1000000);
                say("bench_native_checksum", Long.toHexString(h) + " " + s);
            }
        } catch (Throwable t) {
            say("bench_native_error", t);
        }
        say("bench_done", "1");
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        try {
            Uri uri = data == null ? null : data.getData();
            if (request == REQUEST_OPEN) {
                say("open_result", result + " " + uri);
                if (uri != null) {
                    InputStream in = getContentResolver().openInputStream(uri);
                    byte[] buf = new byte[64];
                    int n = in.read(buf);
                    in.close();
                    say("open_read", n + " bytes: " + new String(buf, 0, Math.max(0, n), "UTF-8").trim());
                }
            } else if (request == REQUEST_SAVE) {
                say("save_result", result + " " + uri);
                if (uri != null) {
                    OutputStream o = getContentResolver().openOutputStream(uri);
                    o.write("tur-probe-saved-content\n".getBytes("UTF-8"));
                    o.close();
                    say("save_written", "24 bytes");
                }
            }
        } catch (Throwable t) {
            say("activity_result_error", t);
        }
    }

    @Override
    public void onBackPressed() {
        backs++;
        say("back_pressed", backs);
        // Not passed on: the activity stays so that the next step can run.
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        say("key_down", keyCode);
        return super.onKeyDown(keyCode, event);
    }

    @Override
    public void onConfigurationChanged(Configuration c) {
        super.onConfigurationChanged(c);
        say("config", c.screenWidthDp + "x" + c.screenHeightDp + " dp");
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        say("focus", hasFocus);
    }

    @Override
    protected void onResume() {
        super.onResume();
        say("lifecycle", "resume");
    }

    @Override
    protected void onPause() {
        super.onPause();
        say("lifecycle", "pause");
    }

    @Override
    protected void onStop() {
        super.onStop();
        say("lifecycle", "stop");
    }

    @Override
    protected void onDestroy() {
        say("lifecycle", "destroy");
        super.onDestroy();
    }
}
