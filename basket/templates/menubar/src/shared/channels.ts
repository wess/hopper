import { defineChannel, defineEvent } from "@basket/ipc";

export const stats = defineChannel<void, { count: number; lastBumped?: string }>("stats");

export const bump = defineChannel<void, { count: number }>("bump");

export const statsUpdated = defineEvent<{ count: number }>("stats:updated");
