// Hopper site — progressive enhancement only. Everything degrades to a static,
// readable page if JS is off.

// ── inline icons (lucide-flavored, matching the app's icon set) ──────────────
const ICONS = {
  grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
  box: '<path d="M21 8 12 3 3 8v8l9 5 9-5z"/><path d="M3 8l9 5 9-5"/><path d="M12 13v8"/>',
  layers: '<path d="M12 2 2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/>',
  disk: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5v14c0 1.7 4 3 9 3s9-1.3 9-3V5"/><path d="M3 12c0 1.7 4 3 9 3s9-1.3 9-3"/>',
  net: '<circle cx="12" cy="5" r="2"/><circle cx="5" cy="19" r="2"/><circle cx="19" cy="19" r="2"/><path d="M12 7v3M11 12l-5 5M13 12l5 5"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>',
  chev: '<path d="m6 9 6 6 6-6"/>',
  pulse: '<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>',
  filter: '<path d="M22 3H2l8 9.5V19l4 2v-8.5L22 3z"/>',
  spark: '<path d="M12 2l2.3 6.8L21 11l-6.7 2.2L12 20l-2.3-6.8L3 11l6.7-2.2L12 2z"/>',
};

for (const el of document.querySelectorAll("[data-ic]")) {
  const paths = ICONS[el.dataset.ic];
  if (!paths) continue;
  el.innerHTML = `<svg viewBox="0 0 24 24" width="100%" height="100%" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="display:block">${paths}</svg>`;
}

// ── theme toggle (persisted, defaults to system) ─────────────────────────────
const root = document.documentElement;
const stored = localStorage.getItem("hopper-theme");
if (stored) {
  root.dataset.theme = stored;
} else if (window.matchMedia("(prefers-color-scheme: light)").matches) {
  root.dataset.theme = "light";
}
document.getElementById("themeToggle")?.addEventListener("click", () => {
  const next = root.dataset.theme === "dark" ? "light" : "dark";
  root.dataset.theme = next;
  localStorage.setItem("hopper-theme", next);
});

// ── nav: scrolled state + mobile menu ────────────────────────────────────────
const nav = document.getElementById("nav");
const onScroll = () => nav.classList.toggle("scrolled", window.scrollY > 12);
onScroll();
addEventListener("scroll", onScroll, { passive: true });

const burger = document.getElementById("hamburger");
burger?.addEventListener("click", () => {
  const open = nav.classList.toggle("open");
  burger.setAttribute("aria-expanded", String(open));
});
for (const a of document.querySelectorAll(".nav-links a")) {
  a.addEventListener("click", () => {
    nav.classList.remove("open");
    burger?.setAttribute("aria-expanded", "false");
  });
}

// ── scroll reveals ───────────────────────────────────────────────────────────
const io = new IntersectionObserver(
  (entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        e.target.classList.add("in");
        io.unobserve(e.target);
      }
    }
  },
  { rootMargin: "0px 0px -8% 0px", threshold: 0.12 },
);
for (const el of document.querySelectorAll(".reveal")) io.observe(el);

// ── card spotlight follows the cursor ────────────────────────────────────────
for (const card of document.querySelectorAll(".card")) {
  card.addEventListener("pointermove", (e) => {
    const r = card.getBoundingClientRect();
    card.style.setProperty("--mx", `${e.clientX - r.left}px`);
    card.style.setProperty("--my", `${e.clientY - r.top}px`);
  });
}

// ── copy-to-clipboard on the brew pills ──────────────────────────────────────
for (const pill of document.querySelectorAll(".brew[data-copy]")) {
  pill.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(pill.dataset.copy);
      pill.classList.add("copied");
      setTimeout(() => pill.classList.remove("copied"), 1600);
    } catch {
      /* clipboard blocked — no-op */
    }
  });
}

// ── streaming "docker compose up" terminal ───────────────────────────────────
const term = document.getElementById("termBody");
if (term) {
  const LINES = [
    { t: "$ docker compose -f compose.yml up -d", c: "t-accent" },
    { t: "[+] Running 6/6", c: "t-dim" },
    { t: " ✔ Network shop_default      Created", c: "t-ok" },
    { t: ' ✔ Volume "shop_pgdata"      Created', c: "t-ok" },
    { t: " ✔ Container shop-db         Started", c: "t-ok" },
    { t: " ✔ Container shop-api        Started", c: "t-ok" },
    { t: " ✔ Container shop-web        Started", c: "t-ok" },
    { t: "", c: "" },
    { t: "$ ", c: "t-accent", cursor: true },
  ];

  let started = false;
  const type = () => {
    let i = 0;
    const step = () => {
      if (i >= LINES.length) return;
      const line = LINES[i];
      const span = document.createElement("span");
      if (line.c) span.className = line.c;
      span.textContent = line.t;
      term.appendChild(span);
      if (line.cursor) {
        const cur = document.createElement("span");
        cur.className = "cursor";
        term.appendChild(cur);
      }
      term.appendChild(document.createTextNode("\n"));
      i++;
      setTimeout(step, i === 1 ? 520 : i < 3 ? 360 : 240);
    };
    step();
  };

  const tio = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting && !started) {
          started = true;
          type();
          tio.disconnect();
        }
      }
    },
    { threshold: 0.4 },
  );
  tio.observe(document.getElementById("terminal"));
}

// ── footer year ──────────────────────────────────────────────────────────────
const yearEl = document.getElementById("year");
if (yearEl) yearEl.textContent = `© ${new Date().getFullYear()}`;
