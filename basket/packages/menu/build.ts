export type MenuItem =
  | { readonly label: string; readonly action: string; readonly shortcut?: string }
  | { readonly separator: true };

export type MenuSection = {
  readonly label: string;
  readonly items: readonly MenuItem[];
};

export type Menu = readonly MenuSection[];

export const section = (label: string, items: readonly MenuItem[]): MenuSection => ({ label, items });

export const item = (label: string, action: string, opts?: { readonly shortcut?: string }): MenuItem => ({
  label,
  action,
  ...(opts?.shortcut !== undefined ? { shortcut: opts.shortcut } : {}),
});

export const separator = (): MenuItem => ({ separator: true });
