const root = document.documentElement;
root.classList.add("enhanced");

for (const link of document.querySelectorAll('a[href^="#"]')) {
  link.addEventListener("click", (event) => {
    const target = document.querySelector(link.getAttribute("href"));
    if (!target) return;
    event.preventDefault();
    target.scrollIntoView({ behavior: "smooth", block: "start" });
  });
}

const hero = document.querySelector(".manual-frame");
if (hero) {
  requestAnimationFrame(() => hero.classList.add("is-active"));
}

const sections = document.querySelectorAll(".field-section");
if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) entry.target.classList.add("is-visible");
      }
    },
    { rootMargin: "0px 0px -12%", threshold: 0.12 },
  );
  for (const section of sections) observer.observe(section);
}
