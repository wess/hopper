// Restrained cinematic motion: staggered load-in for the hero, scroll reveals
// for the rest, and a docs-nav scroll spy. Everything degrades to no-motion.

(() => {
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const reveals = [...document.querySelectorAll("[data-reveal]")];

  if (reduce || !("IntersectionObserver" in window)) {
    reveals.forEach((el) => el.classList.add("in"));
  } else {
    // Hero elements (already near the top) come in immediately, staggered by
    // their data-delay; the rest reveal as they scroll into view.
    const io = new IntersectionObserver(
      (entries, obs) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            e.target.classList.add("in");
            obs.unobserve(e.target);
          }
        }
      },
      { rootMargin: "0px 0px -12% 0px", threshold: 0.08 }
    );
    reveals.forEach((el) => io.observe(el));
    // Kick the above-the-fold ones on the next frame so the transition plays.
    requestAnimationFrame(() =>
      reveals
        .filter((el) => el.getBoundingClientRect().top < window.innerHeight * 0.9)
        .forEach((el) => el.classList.add("in"))
    );
  }

  // A whisper of parallax on the hero window — translate only, transform-only.
  const stage = document.querySelector(".stage .window");
  if (stage && !reduce) {
    let raf = 0;
    window.addEventListener(
      "scroll",
      () => {
        if (raf) return;
        raf = requestAnimationFrame(() => {
          const y = Math.min(window.scrollY, 600);
          stage.style.transform = `translateY(${y * -0.04}px)`;
          raf = 0;
        });
      },
      { passive: true }
    );
  }

  // Docs scroll-spy: highlight the section currently in view.
  const links = [...document.querySelectorAll(".docs-nav a")];
  if (links.length) {
    const map = new Map();
    links.forEach((a) => {
      const id = a.getAttribute("href");
      if (id && id.startsWith("#")) {
        const t = document.querySelector(id);
        if (t) map.set(t, a);
      }
    });
    const spy = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            links.forEach((l) => l.classList.remove("active"));
            map.get(e.target)?.classList.add("active");
          }
        }
      },
      { rootMargin: "-10% 0px -75% 0px" }
    );
    map.forEach((_a, t) => spy.observe(t));
  }
})();
