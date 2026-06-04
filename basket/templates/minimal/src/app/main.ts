import { invoke, subscribe } from "@basket/ipc/client";
import { greet, themeChanged } from "../shared/channels.ts";

const greeting = await invoke(greet, { name: "world" });
const el = document.getElementById("greeting");
if (el) el.textContent = greeting;

subscribe(themeChanged, ({ theme }) => {
  document.body.dataset.theme = theme;
});
