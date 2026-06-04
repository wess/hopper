import { item, type Menu, section, separator } from "@basket/menu";

// Native menu. Standard Edit actions map straight to the OS; everything else
// is an IPC action handled in host/index.ts (`onMenu`) and forwarded to the
// webview as a `runCommand`.
const menu: Menu = [
  section("File", [
    item("Run Container…", "container:run", { shortcut: "CmdOrCtrl+R" }),
    item("Pull Image…", "image:pull", { shortcut: "CmdOrCtrl+P" }),
    separator(),
    item("Clean / Prune…", "system:prune"),
    item("Settings…", "app:settings", { shortcut: "CmdOrCtrl+," }),
    separator(),
    item("Quit", "app:quit", { shortcut: "CmdOrCtrl+Q" }),
  ]),
  section("Edit", [
    item("Undo", "edit:undo", { shortcut: "CmdOrCtrl+Z" }),
    item("Redo", "edit:redo", { shortcut: "CmdOrCtrl+Shift+Z" }),
    separator(),
    item("Cut", "edit:cut", { shortcut: "CmdOrCtrl+X" }),
    item("Copy", "edit:copy", { shortcut: "CmdOrCtrl+C" }),
    item("Paste", "edit:paste", { shortcut: "CmdOrCtrl+V" }),
    item("Select All", "edit:selectall", { shortcut: "CmdOrCtrl+A" }),
  ]),
  section("View", [
    item("Dashboard", "view:dashboard", { shortcut: "CmdOrCtrl+1" }),
    item("Containers", "view:containers", { shortcut: "CmdOrCtrl+2" }),
    item("Images", "view:images", { shortcut: "CmdOrCtrl+3" }),
    item("Volumes", "view:volumes", { shortcut: "CmdOrCtrl+4" }),
    item("Networks", "view:networks", { shortcut: "CmdOrCtrl+5" }),
    separator(),
    item("Command Palette", "view:palette", { shortcut: "CmdOrCtrl+K" }),
    item("Toggle Theme", "theme:toggle", { shortcut: "CmdOrCtrl+Shift+L" }),
  ]),
];

export default menu;
