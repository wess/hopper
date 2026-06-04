import { mkdir, rename, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { type AppId, paths } from "@basket/config";

export type LogLevel = "debug" | "info" | "warn" | "error";

export const LEVELS: Record<LogLevel, number> = { debug: 10, info: 20, warn: 30, error: 40 };

export type LoggerOptions = {
  readonly app: AppId;
  readonly file?: string;
  readonly maxSize?: number;
  readonly keep?: number;
  readonly level?: LogLevel;
  readonly console?: boolean;
};

export type Logger = {
  readonly path: string;
  readonly debug: (message: string, meta?: Record<string, unknown>) => void;
  readonly info: (message: string, meta?: Record<string, unknown>) => void;
  readonly warn: (message: string, meta?: Record<string, unknown>) => void;
  readonly error: (message: string, meta?: Record<string, unknown>) => void;
  readonly child: (bindings: Record<string, unknown>) => Logger;
  readonly flush: () => Promise<void>;
};

const formatLine = (level: LogLevel, message: string, meta?: Record<string, unknown>): string => {
  const ts = new Date().toISOString();
  const payload = meta && Object.keys(meta).length > 0 ? ` ${JSON.stringify(meta)}` : "";
  return `${ts} ${level.toUpperCase().padEnd(5)} ${message}${payload}\n`;
};

const rotate = async (file: string, keep: number): Promise<void> => {
  for (let i = keep - 1; i >= 1; i--) {
    const from = `${file}.${i}`;
    const to = `${file}.${i + 1}`;
    if (await Bun.file(from).exists()) {
      await rename(from, to).catch(() => {});
    }
  }
  if (await Bun.file(file).exists()) {
    await rename(file, `${file}.1`).catch(() => {});
  }
};

export const createLogger = (options: LoggerOptions): Logger => {
  const logDir = paths(options.app).logs;
  const file = join(logDir, options.file ?? "app.log");
  const maxSize = options.maxSize ?? 5_000_000;
  const keep = options.keep ?? 5;
  const minLevel = LEVELS[options.level ?? "info"];
  const mirrorConsole = options.console ?? false;

  let queue: Promise<void> = Promise.resolve();
  let initialized = false;
  let size = 0;

  const ensureReady = async (): Promise<void> => {
    if (initialized) return;
    await mkdir(dirname(file), { recursive: true });
    try {
      const st = await stat(file);
      size = st.size;
    } catch {
      size = 0;
    }
    initialized = true;
  };

  const append = (line: string): void => {
    queue = queue
      .then(async () => {
        await ensureReady();
        if (size + line.length > maxSize) {
          await rotate(file, keep);
          size = 0;
        }
        const handle = Bun.file(file);
        const existing = (await handle.exists()) ? await handle.text() : "";
        await Bun.write(file, existing + line);
        size += line.length;
      })
      .catch(() => {});
  };

  const log =
    (level: LogLevel, bindings: Record<string, unknown>) => (message: string, meta?: Record<string, unknown>) => {
      if (LEVELS[level] < minLevel) return;
      const merged = { ...bindings, ...(meta ?? {}) };
      const line = formatLine(level, message, Object.keys(merged).length > 0 ? merged : undefined);
      if (mirrorConsole) {
        const fn = level === "error" ? console.error : level === "warn" ? console.warn : console.log;
        fn(line.trimEnd());
      }
      append(line);
    };

  const build = (bindings: Record<string, unknown>): Logger => ({
    path: file,
    debug: log("debug", bindings),
    info: log("info", bindings),
    warn: log("warn", bindings),
    error: log("error", bindings),
    child: (more) => build({ ...bindings, ...more }),
    flush: async () => {
      await queue;
    },
  });

  return build({});
};
