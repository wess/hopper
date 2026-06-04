export type NotifyOptions = {
  readonly title: string;
  readonly body: string;
  readonly subtitle?: string;
};

const platform = (): "darwin" | "linux" | "win32" => {
  const p = process.platform;
  return p === "darwin" || p === "linux" || p === "win32" ? p : "linux";
};

const esc = (s: string): string => s.replace(/"/g, '\\"');

const macNotify = async (opts: NotifyOptions): Promise<void> => {
  const sub = opts.subtitle ? ` subtitle "${esc(opts.subtitle)}"` : "";
  const script = `display notification "${esc(opts.body)}" with title "${esc(opts.title)}"${sub}`;
  await Bun.$`osascript -e ${script}`.quiet().nothrow();
};

const linuxNotify = async (opts: NotifyOptions): Promise<void> => {
  const body = opts.subtitle ? `${opts.subtitle}\n${opts.body}` : opts.body;
  await Bun.$`notify-send ${opts.title} ${body}`.quiet().nothrow();
};

const winNotify = async (opts: NotifyOptions): Promise<void> => {
  const t = opts.title.replace(/'/g, "''");
  const b = opts.body.replace(/'/g, "''");
  const ps =
    `[void] [System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms'); ` +
    `$n = New-Object System.Windows.Forms.NotifyIcon; ` +
    `$n.Icon = [System.Drawing.SystemIcons]::Information; ` +
    `$n.Visible = $true; ` +
    `$n.ShowBalloonTip(5000, '${t}', '${b}', 'Info'); ` +
    `Start-Sleep -Seconds 1; $n.Dispose()`;
  await Bun.$`powershell -Command ${ps}`.quiet().nothrow();
};

export const notify = async (opts: NotifyOptions): Promise<void> => {
  if (!opts.title || !opts.body) throw new Error("notify: title and body are required");
  switch (platform()) {
    case "darwin":
      return macNotify(opts);
    case "win32":
      return winNotify(opts);
    default:
      return linuxNotify(opts);
  }
};
