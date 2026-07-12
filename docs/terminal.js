/* ============================================================
   MATBOX — Terminal hero
   Auto-typing intro, then user can take over and type commands.
   Echoes are mocked but realistic (matrix ops, sin, eig, etc.).
   ============================================================ */
(function () {
  const body = document.getElementById("term-body");
  if (!body) return;

  const PROMPT = ">> ";
  const script = [
    { kind: "type", text: "x = [1 2 3; 4 5 6; 7 8 10]", delay: 60 },
    { kind: "out", html: '<span class="term-matrix">[1 2 3; 4 5 6; 7 8 10]</span>' },
    { kind: "pause", ms: 300 },
    { kind: "type", text: "det(x)", delay: 70 },
    { kind: "out", html: '<span class="term-num">ans = -3.0000</span>' },
    { kind: "pause", ms: 250 },
    { kind: "type", text: "[V, D] = eig(x)", delay: 60 },
    { kind: "out", html:
        '<span class="term-num">V =</span><br>' +
        '<span class="term-matrix">  -0.2995  -0.8089   0.5063</span><br>' +
        '<span class="term-matrix">  -0.5881   0.0912  -0.8038</span><br>' +
        '<span class="term-matrix">  -0.7518   0.5815   0.3106</span><br>' +
        '<span class="term-num">D =</span><br>' +
        '<span class="term-matrix">  16.7074    0         0</span><br>' +
        '<span class="term-matrix">    0      -1.1854    0</span><br>' +
        '<span class="term-matrix">    0        0      -0.5220</span>'
    },
    { kind: "pause", ms: 250 },
    { kind: "type", text: "disp('Hello, MATBOX!')", delay: 50 },
    { kind: "out", html: '<span class="term-str">Hello, MATBOX!</span>' },
    { kind: "pause", ms: 400 },
    { kind: "out", html: '<span class="term-ok">✓ try it yourself ↓</span>' },
  ];

  // -------- Helpers
  function el(tag, cls, html) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html !== undefined) e.innerHTML = html;
    return e;
  }
  function append(node) {
    body.appendChild(node);
    body.scrollTop = body.scrollHeight;
  }
  function newInputLine(initialValue = "") {
    const line = el("div", "term-line term-input-line");
    const promptNode = el("span", "term-prompt");
    promptNode.innerHTML = '<span class="arrow">▸</span> ';
    const input = document.createElement("input");
    input.type = "text";
    input.className = "term-input";
    input.spellcheck = false;
    input.autocomplete = "off";
    input.autocapitalize = "off";
    input.value = initialValue;
    line.appendChild(promptNode);
    line.appendChild(input);
    append(line);
    input.focus();
    return input;
  }
  function newCmdLine(text) {
    const line = el("div", "term-line");
    const promptNode = el("span", "term-prompt");
    promptNode.innerHTML = '<span class="arrow">▸</span> ';
    const cmd = el("span", "term-cmd", escapeHtml(text));
    line.appendChild(promptNode);
    line.appendChild(cmd);
    append(line);
  }
  function newOutLine(html) {
    append(el("div", "term-line term-out", html));
  }
  function escapeHtml(s) {
    return s.replace(/[&<>"']/g, (c) => ({
      "&": "&amp;", "<": "&lt;", ">": "&gt;",
      '"': "&quot;", "'": "&#39;",
    }[c]));
  }

  // -------- Mocked REPL evaluator
  function evalLine(raw) {
    const s = raw.trim();
    if (!s) return null;

    // disp('...')
    let m = s.match(/^disp\((['"])(.+?)\1\)$/);
    if (m) return `<span class="term-str">${escapeHtml(m[2])}</span>`;

    // sprintf('...')
    m = s.match(/^sprintf\((['"])(.+?)\1\)$/);
    if (m) return `<span class="term-str">${escapeHtml(m[2])}</span>`;

    // who
    if (s === "who") return "Your variables:<br><span class='term-matrix'>x</span> <span class='term-matrix'>V</span> <span class='term-matrix'>D</span>";

    // clear
    if (s === "clear" || s === "clc") return '<span class="term-ok">✓ workspace cleared</span>';

    // sin/cos/tan of a number
    m = s.match(/^(sin|cos|tan|sqrt|exp|log|abs)\((.+)\)$/);
    if (m) {
      const fn = m[1];
      const arg = parseFloat(m[2]);
      if (!isNaN(arg)) {
        const map = { sin: Math.sin, cos: Math.cos, tan: Math.tan, sqrt: Math.sqrt, exp: Math.exp, log: Math.log, abs: Math.abs };
        const v = map[fn](arg);
        return `<span class="term-num">ans = ${v.toFixed(6)}</span>`;
      }
    }

    // eig(A) form
    m = s.match(/^eig\((.+)\)$/);
    if (m) return `<span class="term-num">ans = 16.7074</span><br><span class="term-num">ans = -1.1854</span><br><span class="term-num">ans = -0.5220</span>`;

    // det
    m = s.match(/^det\((.+)\)$/);
    if (m) return `<span class="term-num">ans = -3.0000</span>`;

    // inv
    m = s.match(/^inv\((.+)\)$/);
    if (m) return '<span class="term-matrix">  -0.7333   0.1333   0.2000</span><br><span class="term-matrix">   0.0667   0.0667  -0.1333</span><br><span class="term-matrix">   0.2000  -0.2000   0.0667</span>';

    // size / length
    m = s.match(/^size\((.+)\)$/);
    if (m) return '<span class="term-num">ans = 3   3</span>';
    m = s.match(/^length\((.+)\)$/);
    if (m) return '<span class="term-num">ans = 3</span>';

    // assignment x = ...
    m = s.match(/^(\w+)\s*=\s*(.+)$/);
    if (m) {
      const lhs = m[1], rhs = m[2];
      // matrix literal
      const mm = rhs.match(/^\[(.+)\]$/);
      if (mm) {
        const rows = mm[1].split(";").map(r => r.trim());
        return `<span class="term-var">${escapeHtml(lhs)}</span> = <span class="term-matrix">[${rows.map(r => rows[0] === r ? escapeHtml(r) : escapeHtml(r)).join("; ")}]</span>`;
      }
      return `<span class="term-var">${escapeHtml(lhs)}</span> = <span class="term-num">${escapeHtml(rhs)}</span>`;
    }

    // help
    if (s === "help" || s === "?") return '<span class="term-out">Type a MATLAB expression. Try: <span class="term-cmd">det(x)</span>, <span class="term-cmd">sin(pi/2)</span>, <span class="term-cmd">[1 2 3] * [1; 2; 3]</span></span>';

    // exit / quit
    if (s === "exit" || s === "quit") return '<span class="term-err">✗ use Ctrl+D to exit (this is a demo)</span>';

    return `<span class="term-err">Unrecognized function or variable: '${escapeHtml(s.split(/[ (]/)[0])}'</span>`;
  }

  // -------- User input
  let busy = false;
  function attachInput(input) {
    input.addEventListener("keydown", (e) => {
      if (busy) return;
      if (e.key === "Enter") {
        const v = input.value;
        input.disabled = true;
        input.style.opacity = "0.5";
        newCmdLine(v);
        const out = evalLine(v);
        if (out) newOutLine(out);
        const next = newInputLine();
        attachInput(next);
      }
    });
  }

  // -------- Auto-run the intro
  let idx = 0;
  function runScript() {
    if (idx >= script.length) {
      // Done — hand over to user
      const input = newInputLine();
      attachInput(input);
      return;
    }
    const step = script[idx++];
    switch (step.kind) {
      case "type": {
        const input = newInputLine();
        busy = true;
        let i = 0;
        const interval = step.delay || 50;
        const ticker = setInterval(() => {
          input.value = step.text.slice(0, ++i);
          if (i >= step.text.length) {
            clearInterval(ticker);
            busy = false;
            input.disabled = true;
            input.style.opacity = "0.4";
            newCmdLine(step.text);
            runScript();
          }
        }, interval);
        break;
      }
      case "out":
        newOutLine(step.html);
        runScript();
        break;
      case "pause":
        setTimeout(runScript, step.ms || 200);
        break;
    }
  }

  // Focus on click anywhere in terminal
  body.addEventListener("click", (e) => {
    const active = body.querySelector(".term-input:not([disabled])");
    if (active) active.focus();
  });

  // Boot
  setTimeout(runScript, 600);
})();
