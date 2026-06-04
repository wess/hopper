import type { CSSProperties, ReactNode } from "react";

export type SidebarLayoutProps = {
  readonly children: ReactNode;
  readonly width?: number;
};

export type SidebarProps = { readonly children: ReactNode };
export type DetailProps = { readonly children: ReactNode };

export const SidebarLayout = ({ children, width = 260 }: SidebarLayoutProps) => (
  <div className="basket-sidebar-layout" style={{ "--basket-sidebar-width": `${width}px` } as CSSProperties}>
    {children}
  </div>
);

SidebarLayout.Sidebar = ({ children }: SidebarProps) => <aside className="basket-sidebar">{children}</aside>;
SidebarLayout.Detail = ({ children }: DetailProps) => <main className="basket-detail">{children}</main>;
