// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The experiment's "pretend" shim (docs/reports/exp-rehost-1.md, section 6).
// Installed as the payload's entry point ONLY in the spoof variants of the
// experiment; a plain re-host changes nothing in the payload.
//
// It tells the main process that it runs on the platform the package was
// built for (TUR_REHOST_PLATFORM=darwin|win32), supplies do-nothing stand-ins
// for the Electron methods that exist only on that platform, writes down
// which stand-ins the application called (TUR_REHOST_LOG), and then loads the
// application's real entry point. It tests one question: does a payload that
// has no Linux code path still come up?
//
// Only the application's own code is told so. Electron's and Node's own
// JavaScript also read process.platform, and call native methods that exist
// on that platform only (run 43544136201: every payload told "darwin" died
// in Electron's Menu.setApplicationMenu). The property is therefore a getter
// that looks at its caller: code inside node: or electron/js2c sees Linux.
//
// Renderer processes and preload scripts are not touched: they see Linux.
'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const url = require('url');

const MAIN = '__TUR_MAIN__';
const IS_ESM = '__TUR_ESM__' === 'true';
const real = process.platform;
const want = process.env.TUR_REHOST_PLATFORM || '';
const logFile = process.env.TUR_REHOST_LOG || '';
const called = new Set();

function note(text) {
  if (!logFile) return;
  try { fs.appendFileSync(logFile, text + '\n'); } catch (error) { /* evidence only */ }
}

function stub(object, label, name, value) {
  if (!object || name in object) return;
  const wrapped = typeof value === 'function'
    ? function (...args) {
      const key = label + '.' + name;
      if (!called.has(key)) { called.add(key); note('stub-called ' + key); }
      return value.apply(this, args);
    }
    : value;
  try {
    Object.defineProperty(object, name, { configurable: true, writable: true, value: wrapped });
  } catch (error) {
    note('stub-failed ' + label + '.' + name + ' ' + error.message);
  }
}

function stubAll(object, label, names, value) {
  for (const name of names) stub(object, label, name, value);
}

const nothing = () => undefined;

if (want && want !== real) {
  const platformGetter = function platformGetter() {
    const limit = Error.stackTraceLimit;
    const holder = {};
    try {
      Error.stackTraceLimit = 2;
      Error.captureStackTrace(holder, platformGetter);
    } finally {
      Error.stackTraceLimit = limit;
    }
    const caller = String(holder.stack || '').split('\n')[1] || '';
    return /\(node:|at node:|electron\/js2c/.test(caller) ? real : want;
  };
  Object.defineProperty(process, 'platform', { get: platformGetter, configurable: true, enumerable: true });
  os.platform = () => want;
  os.type = () => (want === 'darwin' ? 'Darwin' : 'Windows_NT');
  note('platform ' + real + ' -> ' + want);
  const electron = require('electron');
  const { app, systemPreferences, BrowserWindow } = electron;
  const windowPrototype = BrowserWindow && BrowserWindow.prototype;

  if (want === 'darwin') {
    const dock = {};
    stubAll(dock, 'app.dock', ['bounce', 'cancelBounce', 'downloadFinished', 'setBadge', 'hide', 'setMenu', 'setIcon'], nothing);
    stub(dock, 'app.dock', 'getBadge', () => '');
    stub(dock, 'app.dock', 'show', () => Promise.resolve());
    stub(dock, 'app.dock', 'isVisible', () => true);
    stub(dock, 'app.dock', 'getMenu', () => null);
    stub(app, 'app', 'dock', dock);
    stubAll(app, 'app', ['setAboutPanelOptions', 'showAboutPanel', 'hide', 'show', 'setActivationPolicy',
      'setSecureKeyboardEntryEnabled', 'showEmojiPanel', 'setUserActivity', 'invalidateCurrentActivity',
      'resignCurrentActivity', 'updateCurrentActivity', 'enableSandbox'], nothing);
    stub(app, 'app', 'isInApplicationsFolder', () => true);
    stub(app, 'app', 'moveToApplicationsFolder', () => true);
    stub(app, 'app', 'isHidden', () => false);
    stub(app, 'app', 'isSecureKeyboardEntryEnabled', () => false);
    stub(app, 'app', 'getCurrentActivityType', () => '');
    stub(app, 'app', 'isEmojiPanelSupported', () => false);
    stub(app, 'app', 'startAccessingSecurityScopedResource', () => nothing);
    stubAll(systemPreferences, 'systemPreferences', ['getUserDefault', 'setUserDefault', 'removeUserDefault',
      'registerDefaults', 'postNotification', 'postLocalNotification', 'postWorkspaceNotification',
      'unsubscribeNotification', 'unsubscribeLocalNotification', 'unsubscribeWorkspaceNotification'], nothing);
    stubAll(systemPreferences, 'systemPreferences', ['subscribeNotification', 'subscribeLocalNotification',
      'subscribeWorkspaceNotification'], () => 0);
    stub(systemPreferences, 'systemPreferences', 'isSwipeTrackingFromScrollEventsEnabled', () => false);
    stub(systemPreferences, 'systemPreferences', 'getMediaAccessStatus', () => 'granted');
    stub(systemPreferences, 'systemPreferences', 'askForMediaAccess', () => Promise.resolve(true));
    stub(systemPreferences, 'systemPreferences', 'isTrustedAccessibilityClient', () => true);
    stub(systemPreferences, 'systemPreferences', 'canPromptTouchID', () => false);
    stub(systemPreferences, 'systemPreferences', 'promptTouchID', () => Promise.reject(new Error('no Touch ID')));
    stub(systemPreferences, 'systemPreferences', 'getEffectiveAppearance', () => 'light');
    stub(systemPreferences, 'systemPreferences', 'getSystemColor', () => '#007aff');
    stub(systemPreferences, 'systemPreferences', 'getAccentColor', () => '007affff');
    stub(systemPreferences, 'systemPreferences', 'getColor', () => '#000000');
    stub(systemPreferences, 'systemPreferences', 'isDarkMode', () => false);
    stubAll(windowPrototype, 'BrowserWindow', ['setVibrancy', 'setTouchBar', 'setWindowButtonVisibility',
      'setWindowButtonPosition', 'setTrafficLightPosition', 'setSheetOffset', 'setRepresentedFilename',
      'setDocumentEdited', 'setAutoHideCursor', 'selectPreviousTab', 'selectNextTab', 'showAllTabs',
      'mergeAllWindows', 'moveTabToNewWindow', 'toggleTabBar', 'addTabbedWindow', 'setHiddenInMissionControl',
      'invalidateShadow', 'setSimpleFullScreen', 'previewFile', 'closeFilePreview'], nothing);
    stub(windowPrototype, 'BrowserWindow', 'getWindowButtonPosition', () => null);
    stub(windowPrototype, 'BrowserWindow', 'getTrafficLightPosition', () => ({ x: 0, y: 0 }));
    stub(windowPrototype, 'BrowserWindow', 'getRepresentedFilename', () => '');
    stub(windowPrototype, 'BrowserWindow', 'isDocumentEdited', () => false);
    stub(windowPrototype, 'BrowserWindow', 'isSimpleFullScreen', () => false);
    stub(windowPrototype, 'BrowserWindow', 'isHiddenInMissionControl', () => false);
  }

  if (want === 'win32') {
    const home = os.homedir();
    const defaults = {
      APPDATA: path.join(home, '.config'),
      LOCALAPPDATA: path.join(home, '.local', 'share'),
      USERPROFILE: home,
      TEMP: os.tmpdir(),
      TMP: os.tmpdir(),
      ProgramFiles: '/usr',
      SystemRoot: '/usr',
    };
    for (const [key, value] of Object.entries(defaults)) {
      if (!process.env[key]) process.env[key] = value;
    }
    stub(app, 'app', 'setUserTasks', () => true);
    stub(app, 'app', 'setJumpList', () => 'ok');
    stub(app, 'app', 'getJumpListSettings', () => ({ minItems: 0, removedItems: [] }));
    stub(app, 'app', 'setAppUserModelId', nothing);
    stub(systemPreferences, 'systemPreferences', 'getAccentColor', () => '0078d7ff');
    stub(systemPreferences, 'systemPreferences', 'getColor', () => '#000000');
    stub(systemPreferences, 'systemPreferences', 'isAeroGlassEnabled', () => true);
    stubAll(windowPrototype, 'BrowserWindow', ['setThumbnailClip', 'setThumbnailToolTip', 'setAppDetails',
      'setOverlayIcon', 'hookWindowMessage', 'unhookWindowMessage', 'unhookAllWindowMessages',
      'setTitleBarOverlay'], nothing);
    stub(windowPrototype, 'BrowserWindow', 'setThumbarButtons', () => true);
    stub(windowPrototype, 'BrowserWindow', 'isWindowMessageHooked', () => false);
  }
}

const target = path.join(__dirname, MAIN);
note('loading ' + MAIN + (IS_ESM ? ' (ES module)' : ''));
if (IS_ESM) {
  import(url.pathToFileURL(target).href).catch((error) => {
    note('load-failed ' + String(error && error.stack ? error.stack : error).split('\n').slice(0, 4).join(' | '));
    throw error;
  });
} else {
  require(target);
}
