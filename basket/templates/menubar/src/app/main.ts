import { invoke, subscribe } from "@basket/ipc/client";
import { bump, stats, statsUpdated } from "../shared/channels.ts";

const counterEl = document.getElementById("counter");
const lastEl = document.getElementById("last");
const bumpBtn = document.getElementById("bump") as HTMLButtonElement | null;

const render = (count: number, lastBumped?: string) => {
  if (counterEl) counterEl.textContent = String(count);
  if (lastEl) lastEl.textContent = lastBumped ? new Date(lastBumped).toLocaleTimeString() : "never";
};

const initial = await invoke(stats, undefined as unknown as void);
render(initial.count, initial.lastBumped);

subscribe(statsUpdated, ({ count }) => render(count));

bumpBtn?.addEventListener("click", async () => {
  const { count } = await invoke(bump, undefined as unknown as void);
  render(count, new Date().toISOString());
});
