import { on, setMenu } from "butter";
import type { Menu } from "./build.ts";

export const applyMenu = (menu: Menu): void => {
  setMenu(menu);
};

export const onMenu = (action: string, handler: () => void | Promise<void>): void => {
  on(action, () => {
    void handler();
  });
};
