/* ============================================================
   MATBOX — Realtime client-side search
   Indexes all <h2> + <h3> + <p> + <li> nodes in .doc-section,
   builds snippets, supports keyboard nav, scrolls to section.
   ============================================================ */
(function () {
  const input = document.getElementById("docs-search-input");
  const results = document.getElementById("search-results");
  const meta = document.getElementById("search-meta");
  const metaCount = document.getElementById("search-count");
  const metaClear = document.getElementById("search-clear");
  const banner = document.getElementById("no-results-banner");
  const sections = Array.from(document.querySelectorAll(".doc-section"));
  const sidebarLinks = Array.from(document.querySelectorAll(".docs-sidebar a"));

  if (!input || !sections.length) return;

  // -------- Build search index (once)
  const index = [];
  sections.forEach((sec) => {
    const id = sec.id;
    const titleEl = sec.querySelector("h2");
    const title = titleEl ? titleEl.textContent.trim() : id;
    // Concatenate all text from headings, paragraphs, list items, code, tables
    const textNodes = sec.querySelectorAll("h2, h3, h4, p, li, td, th, code");
    let body = "";
    textNodes.forEach((n) => {
      body += " " + n.textContent.trim();
    });
    body = body.replace(/\s+/g, " ").trim();

    index.push({ id, title, body, el: sec });
  });

  let focusedIdx = -1;
  let currentMatches = [];

  // -------- Helpers
  function escapeHtml(s) {
    return s.replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;",
      '"': "&quot;", "'": "&#39;",
    }[c]));
  }
  function escapeReg(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }

  function makeSnippet(body, query, len = 180) {
    const lower = body.toLowerCase();
    const q = query.toLowerCase();
    const pos = lower.indexOf(q);
    if (pos === -1) {
      return body.slice(0, len) + (body.length > len ? "…" : "");
    }
    const start = Math.max(0, pos - 50);
    const end = Math.min(body.length, pos + q.length + 130);
    const prefix = start > 0 ? "…" : "";
    const suffix = end < body.length ? "…" : "";
    const slice = body.slice(start, end);
    const re = new RegExp("(" + escapeReg(query) + ")", "gi");
    return prefix + escapeHtml(slice).replace(re, "<mark>$1</mark>") + suffix;
  }

  function showSectionsForQuery(q) {
    if (!q) {
      sections.forEach((s) => s.classList.remove("hidden"));
      banner.classList.remove("active");
      sidebarLinks.forEach((a) => a.closest("li")?.style.setProperty("display", ""));
      return [];
    }
    const ql = q.toLowerCase();
    const matches = [];
    sections.forEach((s) => {
      const entry = index.find((e) => e.el === s);
      const hit =
        entry.title.toLowerCase().includes(ql) ||
        entry.body.toLowerCase().includes(ql);
      if (hit) {
        s.classList.remove("hidden");
        matches.push(entry);
        // highlight in sidebar
        const link = sidebarLinks.find((a) => a.getAttribute("href") === "#" + s.id);
        if (link) link.closest("li")?.style.setProperty("display", "");
      } else {
        s.classList.add("hidden");
        const link = sidebarLinks.find((a) => a.getAttribute("href") === "#" + s.id);
        if (link) link.closest("li")?.style.setProperty("display", "none");
      }
    });
    if (!matches.length) {
      banner.classList.add("active");
      banner.innerHTML =
        '<strong>No results for "' + escapeHtml(q) + '".</strong> ' +
        'Try a shorter query or browse the sidebar.';
    } else {
      banner.classList.remove("active");
    }
    return matches;
  }

  function renderResults(matches, q) {
    if (!q) {
      results.classList.remove("active");
      results.innerHTML = "";
      metaCount.textContent = sections.length + " sections indexed";
      return;
    }
    if (!matches.length) {
      results.classList.add("active");
      results.innerHTML = '<div class="search-empty">No matches found.</div>';
      metaCount.textContent = "0 results";
      return;
    }
    const limited = matches.slice(0, 12);
    const html = limited
      .map((m, i) => {
        const snip = makeSnippet(m.body, q);
        const titleMarked = escapeHtml(m.title).replace(
          new RegExp("(" + escapeReg(q) + ")", "gi"),
          "<mark>$1</mark>"
        );
        return (
          '<a class="search-result' + (i === 0 ? " focused" : "") +
          '" data-idx="' + i + '" href="#' + m.id + '">' +
          '<div class="title">' + titleMarked + "</div>" +
          '<div class="snippet">' + snip + "</div>" +
          "</a>"
        );
      })
      .join("");
    results.innerHTML = html;
    results.classList.add("active");
    metaCount.textContent =
      matches.length + (matches.length === 1 ? " section" : " sections") +
      " match" + (matches.length === 1 ? "" : "es");

    // wire up clicks
    results.querySelectorAll(".search-result").forEach((el) => {
      el.addEventListener("click", (e) => {
        const target = document.getElementById(el.getAttribute("href").slice(1));
        if (target) {
          target.scrollIntoView({ behavior: "smooth", block: "start" });
        }
        results.classList.remove("active");
        input.blur();
      });
      el.addEventListener("mouseenter", () => {
        results.querySelectorAll(".search-result").forEach((r) => r.classList.remove("focused"));
        el.classList.add("focused");
        focusedIdx = parseInt(el.dataset.idx, 10);
      });
    });
  }

  function runSearch(q) {
    currentMatches = showSectionsForQuery(q);
    renderResults(currentMatches, q);
  }

  // -------- Wire up
  let debounce;
  input.addEventListener("input", () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => runSearch(input.value.trim()), 60);
  });

  input.addEventListener("focus", () => {
    if (input.value.trim()) results.classList.add("active");
  });
  input.addEventListener("keydown", (e) => {
    const items = Array.from(results.querySelectorAll(".search-result"));
    if (e.key === "ArrowDown") {
      e.preventDefault();
      focusedIdx = Math.min(focusedIdx + 1, items.length - 1);
      items.forEach((r, i) => r.classList.toggle("focused", i === focusedIdx));
      items[focusedIdx]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusedIdx = Math.max(focusedIdx - 1, 0);
      items.forEach((r, i) => r.classList.toggle("focused", i === focusedIdx));
      items[focusedIdx]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      if (focusedIdx >= 0 && items[focusedIdx]) {
        e.preventDefault();
        items[focusedIdx].click();
      } else if (currentMatches[0]) {
        e.preventDefault();
        const target = document.getElementById(currentMatches[0].id);
        target?.scrollIntoView({ behavior: "smooth", block: "start" });
        results.classList.remove("active");
        input.blur();
      }
    } else if (e.key === "Escape") {
      input.value = "";
      runSearch("");
      results.classList.remove("active");
    }
  });

  metaClear.addEventListener("click", () => {
    input.value = "";
    runSearch("");
    results.classList.remove("active");
    input.focus();
  });

  document.addEventListener("click", (e) => {
    if (!e.target.closest(".docs-search") && !e.target.closest(".search-results")) {
      results.classList.remove("active");
    }
  });

  // Keyboard shortcut: "/" focuses search
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement.tagName !== "INPUT" && document.activeElement.tagName !== "TEXTAREA") {
      e.preventDefault();
      input.focus();
      input.select();
    }
  });

  // -------- Sidebar active link on scroll
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          const id = entry.target.id;
          sidebarLinks.forEach((a) => {
            a.classList.toggle("active", a.getAttribute("href") === "#" + id);
          });
        }
      });
    },
    { rootMargin: "-30% 0px -60% 0px" }
  );
  sections.forEach((s) => observer.observe(s));

  // -------- Mobile sidebar toggle
  const toggle = document.getElementById("docs-mobile-toggle");
  const sidebar = document.querySelector(".docs-sidebar");
  if (toggle && sidebar) {
    toggle.addEventListener("click", () => {
      sidebar.classList.toggle("open");
      toggle.textContent = sidebar.classList.contains("open") ? "✕" : "☰";
    });
    sidebar.querySelectorAll("a").forEach((a) =>
      a.addEventListener("click", () => {
        sidebar.classList.remove("open");
        if (toggle) toggle.textContent = "☰";
      })
    );
  }

  // -------- Initial state
  metaCount.textContent = sections.length + " sections indexed";
})();
