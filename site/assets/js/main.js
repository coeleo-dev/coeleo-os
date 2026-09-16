/* Coeleo OS landing page — progressive enhancement only.
   The page is fully readable with this file blocked. */
(() => {
  "use strict";

  /* ── Sticky nav: border once scrolled ─────────────────────── */
  const nav = document.getElementById("nav");
  const links = [...document.querySelectorAll('.nav__menu a[href^="#"]')];
  const sections = links
    .map((a) => document.querySelector(a.getAttribute("href")))
    .filter(Boolean);

  const onScroll = () => nav?.classList.toggle("is-stuck", window.scrollY > 8);
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  /* ── Highlight the section currently on screen ────────────── */
  if ("IntersectionObserver" in window && sections.length) {
    const ratios = new Map();
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) ratios.set(e.target.id, e.isIntersecting ? e.intersectionRatio : 0);
        let best = null;
        let bestRatio = 0;
        for (const [id, r] of ratios) if (r > bestRatio) [best, bestRatio] = [id, r];
        for (const a of links)
          a.classList.toggle("is-active", best !== null && a.getAttribute("href") === `#${best}`);
      },
      { rootMargin: "-84px 0px -55% 0px", threshold: [0, 0.25, 0.5, 1] },
    );
    for (const s of sections) io.observe(s);
  }

  /* ── Mobile menu ──────────────────────────────────────────── */
  const toggle = document.getElementById("navToggle");
  const menu = document.getElementById("navMenu");
  const setMenu = (open) => {
    menu?.classList.toggle("is-open", open);
    toggle?.setAttribute("aria-expanded", String(open));
  };
  toggle?.addEventListener("click", () => setMenu(!menu.classList.contains("is-open")));
  menu?.addEventListener("click", (e) => {
    if (e.target instanceof HTMLAnchorElement) setMenu(false);
  });

  /* ── Screenshot lightbox ──────────────────────────────────── */
  const box = document.getElementById("lightbox");
  const boxImg = document.getElementById("lbImg");
  const shots = [...document.querySelectorAll("#gallery .shot img")];

  if (box && boxImg && shots.length) {
    let index = 0;
    let lastFocus = null;

    const show = (i) => {
      index = (i + shots.length) % shots.length;
      boxImg.src = shots[index].currentSrc || shots[index].src;
      boxImg.alt = shots[index].alt;
    };

    const open = (i) => {
      lastFocus = document.activeElement;
      show(i);
      box.hidden = false;
      document.body.style.overflow = "hidden";
      document.getElementById("lbClose")?.focus();
    };

    const close = () => {
      box.hidden = true;
      boxImg.removeAttribute("src");
      document.body.style.overflow = "";
      if (lastFocus instanceof HTMLElement) lastFocus.focus();
    };

    shots.forEach((img, i) => img.closest("button")?.addEventListener("click", () => open(i)));
    document.getElementById("lbClose")?.addEventListener("click", close);
    document.getElementById("lbPrev")?.addEventListener("click", () => show(index - 1));
    document.getElementById("lbNext")?.addEventListener("click", () => show(index + 1));
    box.addEventListener("click", (e) => {
      if (e.target === box) close();
    });
    window.addEventListener("keydown", (e) => {
      if (box.hidden) return;
      if (e.key === "Escape") close();
      else if (e.key === "ArrowLeft") show(index - 1);
      else if (e.key === "ArrowRight") show(index + 1);
    });
  }

  /* ── Copy-to-clipboard on command blocks ──────────────────── */
  for (const pre of document.querySelectorAll("pre.code")) {
    if (!navigator.clipboard) break;

    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "copy";
    btn.textContent = "Copy";
    btn.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(pre.innerText);
        btn.textContent = "Copied";
      } catch {
        btn.textContent = "Failed";
      }
      setTimeout(() => {
        btn.textContent = "Copy";
      }, 1600);
    });

    const holder = document.createElement("div");
    holder.className = "pre-wrap";
    pre.replaceWith(holder);
    holder.append(pre, btn);
  }
})();
