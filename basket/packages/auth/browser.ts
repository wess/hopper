export const openBrowser = async (url: string): Promise<void> => {
  const p = process.platform;
  if (p === "darwin") {
    await Bun.$`open ${url}`.quiet();
  } else if (p === "win32") {
    await Bun.$`cmd /c start "" ${url}`.quiet();
  } else {
    await Bun.$`xdg-open ${url}`.quiet();
  }
};
