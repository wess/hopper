# Hopper site

The marketing site for Hopper. Static, dependency-free — plain HTML/CSS/JS.

## Run

```bash
rustc site/serve.rs -o /tmp/hoppersite && SITE_DIR=site /tmp/hoppersite        # → http://localhost:3000
```

Or just open `site/index.html` in a browser — there's no build step.

## Files

- `index.html` — markup + inline brand/SVG marks
- `styles.css` — design system (dark/light themes, the app-mock UI, animations)
- `main.js` — theme toggle, scroll reveals, copy-to-clipboard, the streamed
  `docker compose up` terminal, icon injection
- `serve.ts` — tiny Bun static server

## Notes

- Fonts (Bricolage Grotesque, Hanken Grotesk, JetBrains Mono) load from Google
  Fonts; everything else is local.
- The hero centerpiece is a CSS recreation of Hopper's Containers view, mirroring
  the real compose grouping, stack badges, and status dots — no screenshot needed.
- Light/dark follows the system on first visit, then remembers your choice.
- Download links (macOS `.dmg`, Linux `.AppImage`, Windows `.exe`) point at
  `github.com/wess/hopper/releases`; swap them when releases land.
