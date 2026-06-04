import type { ReactNode } from "react";

export type TitlebarProps = {
  readonly children?: ReactNode;
  readonly trafficLights?: boolean;
};

export type TitlebarTitleProps = { readonly children: ReactNode };
export type TitlebarActionsProps = { readonly children: ReactNode };

export const Titlebar = ({ children, trafficLights = true }: TitlebarProps) => (
  <div className="basket-titlebar">
    {trafficLights ? <span data-traffic-spacer /> : null}
    {children}
  </div>
);

Titlebar.Title = ({ children }: TitlebarTitleProps) => <div className="basket-titlebar-title">{children}</div>;

Titlebar.Actions = ({ children }: TitlebarActionsProps) => (
  <div className="basket-titlebar-actions" data-no-drag>
    {children}
  </div>
);
