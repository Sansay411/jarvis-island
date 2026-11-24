// Light typography for worked maths written by the model: turns plain ASCII
// and the common LaTeX commands into Unicode (x^2 → x², \frac{a}{b} → a/b,
// <= → ≤). No layout engine: the lines stay one-dimensional text.

const SUP: Record<string, string> = {
  "0": "⁰", "1": "¹", "2": "²", "3": "³", "4": "⁴", "5": "⁵", "6": "⁶", "7": "⁷", "8": "⁸", "9": "⁹",
  "+": "⁺", "-": "⁻", "=": "⁼", "(": "⁽", ")": "⁾", n: "ⁿ", i: "ⁱ", x: "ˣ", y: "ʸ",
};
const SUB: Record<string, string> = {
  "0": "₀", "1": "₁", "2": "₂", "3": "₃", "4": "₄", "5": "₅", "6": "₆", "7": "₇", "8": "₈", "9": "₉",
  "+": "₊", "-": "₋", "=": "₌", "(": "₍", ")": "₎", n: "ₙ", i: "ᵢ", x: "ₓ",
};

const COMMANDS: Record<string, string> = {
  cdot: "·", times: "×", div: "÷", pm: "±", mp: "∓", le: "≤", leq: "≤", ge: "≥", geq: "≥", ne: "≠", neq: "≠",
  approx: "≈", equiv: "≡", infty: "∞", to: "→", rightarrow: "→", Rightarrow: "⇒", implies: "⇒", iff: "⇔",
  leftarrow: "←", in: "∈", notin: "∉", subset: "⊂", cup: "∪", cap: "∩", forall: "∀", exists: "∃",
  sum: "∑", prod: "∏", int: "∫", partial: "∂", nabla: "∇", circ: "°", degree: "°", angle: "∠", perp: "⊥",
  alpha: "α", beta: "β", gamma: "γ", delta: "δ", Delta: "Δ", epsilon: "ε", theta: "θ", lambda: "λ", mu: "μ",
  pi: "π", Pi: "Π", rho: "ρ", sigma: "σ", Sigma: "Σ", tau: "τ", phi: "φ", Phi: "Φ", omega: "ω", Omega: "Ω",
  quad: " ", qquad: "  ", ldots: "…", cdots: "⋯", dots: "…", log: "log", ln: "ln", sin: "sin", cos: "cos",
  tan: "tan", lim: "lim", max: "max", min: "min",
};

/** Maps every character or returns null when one has no raised/lowered form. */
function script(text: string, table: Record<string, string>): string | null {
  let out = "";
  for (const ch of text) {
    const m = table[ch];
    if (m === undefined) return null;
    out += m;
  }
  return out;
}

/** Brackets anything but a lone number or name, so `a+b` over `2a` reads `(a+b)/(2a)`. */
function group(s: string): string {
  const t = s.trim();
  return /^(?:[\p{N}.,]+|\p{L}+)$/u.test(t) ? t : `(${t})`;
}

/** Innermost `\cmd{a}{b}` / `\cmd{a}` replacement, applied until stable. */
function replaceBraced(s: string): string {
  let prev = "";
  let out = s;
  for (let i = 0; i < 8 && out !== prev; i++) {
    prev = out;
    out = out
      .replace(/\\[dt]?frac\s*\{([^{}]*)\}\s*\{([^{}]*)\}/g, (_, a: string, b: string) => `${group(a)}/${group(b)}`)
      .replace(/\\sqrt\s*\[([^\]]*)\]\s*\{([^{}]*)\}/g, (_, n: string, a: string) => `${script(n, SUP) ?? `(${n})`}√${group(a)}`)
      .replace(/\\sqrt\s*\{([^{}]*)\}/g, (_, a: string) => `√${group(a)}`)
      .replace(/\\(?:text|mathrm|mathbf|operatorname|mathit)\s*\{([^{}]*)\}/g, "$1");
  }
  return out;
}

export function prettyMath(input: string): string {
  let s = input.trim();
  // Inline-maths fences the model sometimes leaves in.
  s = s.replace(/^\$\$?|\$\$?$/g, "").replace(/\\\(|\\\)|\\\[|\\\]/g, "");
  s = s.replace(/\\left|\\right/g, "").replace(/\\[,;!:]/g, " ");
  s = replaceBraced(s);
  s = s.replace(/\\([A-Za-z]+)/g, (m, name: string) => COMMANDS[name] ?? m);
  s = s.replace(/sqrt\s*\(([^()]*)\)/g, (_, a: string) => `√${group(a)}`);

  const raise = (table: Record<string, string>, mark: string) =>
    (_m: string, braced: string | undefined, paren: string | undefined, bare: string | undefined): string => {
      const body = braced ?? paren ?? bare ?? "";
      return script(body, table) ?? `${mark}${braced !== undefined || paren !== undefined ? `(${body})` : body}`;
    };
  s = s.replace(/\^(?:\{([^{}]*)\}|\(([^()]*)\)|(-?[0-9]+|[A-Za-z]))/g, raise(SUP, "^"));
  s = s.replace(/_(?:\{([^{}]*)\}|\(([^()]*)\)|([0-9]+|[a-z]))/g, raise(SUB, "_"));
  s = s.replace(/\{|\}/g, "");

  s = s
    .replace(/<=>/g, "⇔")
    .replace(/<=/g, "≤")
    .replace(/>=/g, "≥")
    .replace(/!=/g, "≠")
    .replace(/=>/g, "⇒")
    .replace(/->/g, "→")
    .replace(/\+-/g, "±")
    .replace(/~=/g, "≈")
    .replace(/\s*\*\s*/g, " · ")
    .replace(/([\p{L}\p{N})])\s*-\s*(?=[\p{L}\p{N}(√])/gu, "$1 − ")
    .replace(/(^|[\s(=])-(?=[\p{L}\p{N}(√])/gu, "$1−");
  return s.replace(/[ \t]{2,}/g, " ").trim();
}
