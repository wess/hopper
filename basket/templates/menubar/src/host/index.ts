import { defineConfig } from "@basket/config";
import { emit, handle } from "@basket/ipc";
import { createStore } from "@basket/store";
import { createTray } from "@basket/tray";
import { bump, stats, statsUpdated } from "../shared/channels.ts";

const config = defineConfig({
  app: {
    name: "{{name}}",
    id: "com.example.{{name}}",
  },
});

const settings = createStore("state", {
  app: config.app,
  defaults: { count: 0, lastBumped: undefined as string | undefined },
});

const incr = (): { count: number } => {
  const next = (settings.get<number>("count") ?? 0) + 1;
  settings.set("count", next);
  settings.set("lastBumped", new Date().toISOString());
  emit(statsUpdated, { count: next });
  return { count: next };
};

const tray = createTray({
  title: `${config.app.name}: 0`,
  tooltip: config.app.name,
  items: [
    { label: "Bump", action: "tray:bump" },
    { label: "Reset", action: "tray:reset" },
    { separator: true },
    { label: "Quit", action: "tray:quit" },
  ],
});

tray.onAction("tray:bump", () => {
  const { count } = incr();
  tray.set({
    title: `${config.app.name}: ${count}`,
    tooltip: config.app.name,
    items: [
      { label: "Bump", action: "tray:bump" },
      { label: "Reset", action: "tray:reset" },
      { separator: true },
      { label: "Quit", action: "tray:quit" },
    ],
  });
});

tray.onAction("tray:reset", () => {
  settings.set("count", 0);
  settings.set("lastBumped", undefined);
  emit(statsUpdated, { count: 0 });
  tray.set({
    title: `${config.app.name}: 0`,
    tooltip: config.app.name,
    items: [
      { label: "Bump", action: "tray:bump" },
      { label: "Reset", action: "tray:reset" },
      { separator: true },
      { label: "Quit", action: "tray:quit" },
    ],
  });
});

tray.onAction("tray:quit", () => process.exit(0));

handle(stats, () => ({
  count: settings.get<number>("count") ?? 0,
  lastBumped: settings.get<string>("lastBumped"),
}));

handle(bump, () => incr());

console.log(`[${config.app.name}] menubar ready.`);
