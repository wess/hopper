import { defineChannel, defineEvent } from "@basket/ipc";

export const greet = defineChannel<{ name: string }, string>("greet");

export const themeChanged = defineEvent<{ theme: "light" | "dark" }>("theme:changed");
