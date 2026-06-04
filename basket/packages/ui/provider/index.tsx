import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";
import { STYLES } from "../styles.ts";

export type UiTheme = "light" | "dark" | "auto";

export type BasketAppInfo = {
  readonly name: string;
  readonly id?: string;
  readonly version?: string;
};

type Ctx = {
  app: BasketAppInfo;
  theme: UiTheme;
  resolved: "light" | "dark";
  setTheme: (t: UiTheme) => void;
};

const Context = createContext<Ctx | undefined>(undefined);

const detectSystem = (): "light" | "dark" =>
  typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

export type BasketProviderProps = {
  readonly app: BasketAppInfo;
  readonly theme?: UiTheme;
  readonly children: ReactNode;
};

export const BasketProvider = ({ app, theme: initial, children }: BasketProviderProps) => {
  const [theme, setTheme] = useState<UiTheme>(initial ?? "auto");
  const [system, setSystem] = useState<"light" | "dark">("light");

  useEffect(() => {
    if (typeof window === "undefined") return;
    setSystem(detectSystem());
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const fn = () => setSystem(mq.matches ? "dark" : "light");
    mq.addEventListener("change", fn);
    return () => mq.removeEventListener("change", fn);
  }, []);

  const resolved = theme === "auto" ? system : theme;

  useEffect(() => {
    if (typeof document === "undefined") return;
    document.documentElement.setAttribute("data-basket-ui", "");
    document.documentElement.setAttribute("data-basket-theme", resolved);
  }, [resolved]);

  const value = useMemo<Ctx>(() => ({ app, theme, resolved, setTheme }), [app, theme, resolved]);

  return (
    <Context.Provider value={value}>
      <style dangerouslySetInnerHTML={{ __html: STYLES }} />
      {children}
    </Context.Provider>
  );
};

export const useBasket = (): Ctx => {
  const c = useContext(Context);
  if (!c) throw new Error("useBasket() must be called inside <BasketProvider>");
  return c;
};

export const useTheme = () => {
  const { theme, resolved, setTheme } = useBasket();
  return { theme, resolved, setTheme };
};
