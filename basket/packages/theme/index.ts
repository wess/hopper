import { on } from "butter";

export type SystemTheme = "light" | "dark";
export type Theme = "light" | "dark" | "system";

const detect = async (): Promise<SystemTheme> => {
  try {
    if (process.platform === "darwin") {
      const r = await Bun.$`defaults read -g AppleInterfaceStyle`.quiet().nothrow();
      return r.exitCode === 0 && r.text().trim().toLowerCase() === "dark" ? "dark" : "light";
    }
    if (process.platform === "linux") {
      const r = await Bun.$`gsettings get org.gnome.desktop.interface color-scheme`.quiet().nothrow();
      if (r.exitCode === 0 && r.text().includes("dark")) return "dark";
      if ((Bun.env.GTK_THEME ?? "").toLowerCase().includes("dark")) return "dark";
      return "light";
    }
    if (process.platform === "win32") {
      const r =
        await Bun.$`powershell -Command (Get-ItemProperty -Path HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize).AppsUseLightTheme`
          .quiet()
          .nothrow();
      return r.exitCode === 0 && r.text().trim() === "0" ? "dark" : "light";
    }
  } catch {
    /* fall through */
  }
  return "light";
};

export const getSystemTheme = (): Promise<SystemTheme> => detect();

type ChangeHandler = (theme: SystemTheme) => void;
const listeners: ChangeHandler[] = [];
let wired = false;

const ensureWired = (): void => {
  if (wired) return;
  wired = true;
  on("theme:changed", (data) => {
    const theme = (data as { theme?: SystemTheme })?.theme;
    if (theme === "light" || theme === "dark") {
      for (const fn of listeners) fn(theme);
    }
  });
};

export const onThemeChange = (handler: ChangeHandler): (() => void) => {
  ensureWired();
  listeners.push(handler);
  return () => {
    const i = listeners.indexOf(handler);
    if (i >= 0) listeners.splice(i, 1);
  };
};

export type ThemeManagerOptions = {
  readonly initial?: Theme;
  readonly onResolved?: (theme: SystemTheme) => void;
};

export type ThemeManager = {
  readonly get: () => Theme;
  readonly resolved: () => SystemTheme;
  readonly set: (theme: Theme) => void;
  readonly subscribe: (fn: (resolved: SystemTheme, raw: Theme) => void) => () => void;
};

export const createThemeManager = (options: ThemeManagerOptions = {}): ThemeManager => {
  let current: Theme = options.initial ?? "system";
  let resolved: SystemTheme = "light";
  const subs: ((r: SystemTheme, raw: Theme) => void)[] = [];

  const notify = () => {
    options.onResolved?.(resolved);
    for (const s of subs) s(resolved, current);
  };

  // Initial detect
  void detect().then((t) => {
    if (current === "system") {
      resolved = t;
      notify();
    }
  });

  // System changes
  onThemeChange((t) => {
    if (current === "system") {
      resolved = t;
      notify();
    }
  });

  return {
    get: () => current,
    resolved: () => resolved,
    set: (theme) => {
      current = theme;
      if (theme === "system") {
        void detect().then((t) => {
          resolved = t;
          notify();
        });
      } else {
        resolved = theme;
        notify();
      }
    },
    subscribe: (fn) => {
      subs.push(fn);
      return () => {
        const i = subs.indexOf(fn);
        if (i >= 0) subs.splice(i, 1);
      };
    },
  };
};
