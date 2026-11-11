// The landing page's few interactive pieces. The page is complete without this script.

const root = document.documentElement;

// Theme: dark by default, remembered per browser under the same key as the app.
document.querySelectorAll<HTMLButtonElement>("[data-theme-toggle]").forEach((button) =>
  button.addEventListener("click", () => {
    const next = root.classList.contains("light") ? "dark" : "light";
    root.className = next;
    try {
      localStorage.setItem("hyperbeam:theme", next);
    } catch {
      // Private windows may refuse; the choice still applies on this page.
    }
  }),
);

// Mobile menu.
const menuButton = document.querySelector<HTMLButtonElement>("[data-menu-toggle]");
const menu = document.getElementById("mobile-nav");
menuButton?.addEventListener("click", () => {
  const open = menu!.hidden;
  menu!.hidden = !open;
  menuButton.setAttribute("aria-expanded", String(open));
});
menu?.addEventListener("click", (e) => {
  if ((e.target as HTMLElement).closest("a")) {
    menu.hidden = true;
    menuButton?.setAttribute("aria-expanded", "false");
  }
});

// Tabs.
document.querySelectorAll<HTMLElement>("[data-tabs]").forEach((group) => {
  const tabs = [...group.querySelectorAll<HTMLButtonElement>("[role=tab]")];
  const select = (tab: HTMLButtonElement) => {
    for (const t of tabs) {
      const selected = t === tab;
      t.setAttribute("aria-selected", String(selected));
      t.tabIndex = selected ? 0 : -1;
      document.getElementById(t.getAttribute("aria-controls")!)!.hidden = !selected;
    }
  };
  tabs.forEach((tab, i) => {
    tab.addEventListener("click", () => select(tab));
    tab.addEventListener("keydown", (e) => {
      const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
      if (!step) return;
      const next = tabs[(i + step + tabs.length) % tabs.length];
      select(next);
      next.focus();
    });
  });
});

// Copy buttons.
document.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach((button) =>
  button.addEventListener("click", async () => {
    const label = button.querySelector("span");
    try {
      await navigator.clipboard.writeText(button.dataset.copy ?? "");
      if (label) label.textContent = "Copied";
    } catch {
      if (label) label.textContent = "Press Ctrl+C";
    }
    setTimeout(() => label && (label.textContent = "Copy"), 1600);
  }),
);

// The hero demo plays its session once it scrolls into view, then rests as a finished session.
const demo = document.querySelector<HTMLElement>("[data-demo]");
if (demo && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
  const steps = [...demo.querySelectorAll<HTMLElement>("[data-step]")];
  const order = [...new Set(steps.map((s) => Number(s.dataset.step)))].sort((a, b) => a - b);
  demo.classList.add("demo-live");
  const play = () => {
    order.forEach((n, i) =>
      setTimeout(() => steps.filter((s) => Number(s.dataset.step) === n).forEach((s) => s.classList.add("shown")), 350 + i * 650),
    );
  };
  const observer = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting)) {
      observer.disconnect();
      play();
    }
  }, { threshold: 0.25 });
  observer.observe(demo);
}
