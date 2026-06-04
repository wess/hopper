import { defineConfig, paths } from "@basket/config";
import { handle, emit } from "@basket/ipc";
import { applyMenu, item, onMenu, section, separator } from "@basket/menu";
import { createStore } from "@basket/store";
import { mainWindow } from "@basket/window";
import menu from "./menu.ts";
import { greet, themeChanged } from "../shared/channels.ts";

const config = defineConfig({
  app: {
    name: "{{name}}",
    id: "com.example.{{name}}",
  },
});

const settings = createStore("settings", {
  app: config.app,
  defaults: { theme: "light" },
});

const win = mainWindow({
  defaults: { width: 1000, height: 700, title: config.app.name },
  store: settings,
  storeKey: "main",
});

applyMenu(menu);

handle(greet, ({ name }) => `Hello, ${name}! You are running ${config.app.name}.`);

onMenu("theme:toggle", () => {
  const next = settings.get<"light" | "dark">("theme") === "dark" ? "light" : "dark";
  settings.set("theme", next);
  emit(themeChanged, { theme: next });
});

onMenu("quit", () => {
  win.save();
  process.exit(0);
});

console.log(`[${config.app.name}] host ready. Data dir: ${paths(config.app).data}`);
