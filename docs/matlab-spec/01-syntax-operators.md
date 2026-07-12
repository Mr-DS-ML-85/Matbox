# MATBOX Language Spec — 01: Lexical Structure, Operators, and Expression/Statement Semantics

> **Clean-room note.** This document describes the *documented behavior* of the MATLAB
> language as published on public MathWorks documentation (`mathworks.com/help`) and other
> public references. It is written from scratch, in our own words, for the MATBOX
> reimplementation. No MathWorks source code was consulted or reproduced. Where exact
> user-visible text matters for compatibility (error/warning messages), only the
> user-facing strings — which are public and observable — are recorded.
>
> Behavior described targets modern MATLAB (roughly R2016b–R2024b). Version-sensitive
> features are flagged inline.

---

## 1. Lexical Structure

### 1.1 Character set and encoding
- Source is Unicode text. Identifiers are ASCII-letter-led in practice.
- Identifiers: start with a letter, followed by letters, digits, or underscores. Maximum
  significant length is given by `namelengthmax` (63 in current releases). Names are
  **case-sensitive** (`X` ≠ `x`). Names beginning with an underscore or digit are invalid.

### 1.2 Whitespace significance
Whitespace is **usually** insignificant between tokens, but is **significant** in three
contexts, which an implementer must get right:

1. **Matrix / array literals inside `[ ]` and `{ }`.** Spaces (and commas) separate
   *columns*; semicolons (and newlines) separate *rows*. Therefore `[1 -1]` is a
   1×2 row `[1, -1]`, while `[1 - 1]` is the 1×1 scalar `0`, and `[1- 1]` is also `0`.
   The rule the lexer applies: inside brackets, a binary operator with a space *before* it
   and no space *after* it (e.g. `-1`) is treated as a **unary** prefix beginning a new
   element; balanced spacing (` - `) or no spacing (`-`) keeps it **binary**. This is the
   single most error-prone whitespace rule in the language.
2. **Command syntax vs. function syntax** (see §6): a space after an identifier can switch
   a statement from an expression into a command invocation.
3. **Transpose vs. string quote disambiguation** (see §3.4): whether `'` is the transpose
   operator or the opening of a char literal depends on the preceding token, not on
   whitespace directly, but spacing interacts with it.

Outside these contexts, `a+b`, `a + b`, and `a  +  b` are identical.

### 1.3 Comments
| Form | Meaning |
|------|---------|
| `% ...` | Single-line comment: from `%` to end of line. |
| `%{` … `%}` | Block comment spanning multiple lines. |
| `... comment text` | Everything after `...` on the same line is ignored (continuation + trailing comment, see §1.4). |
| `%% ...` | Section ("code cell") divider in the Editor. Semantically an ordinary `%` comment; only tooling treats it specially. |

**Block comment rules (strict):** `%{` and `%}` must each appear **alone on their own
line** (only whitespace allowed around them), and `%{` must be immediately followed on
subsequent lines by the commented block, terminated by a line containing only `%}`. If any
other non-whitespace text shares the line with `%{`, it is **not** treated as a block-comment
opener — it degrades to an ordinary single-line `%` comment. Block comments nest.

There is no C-style `/* */`. `%` is also overloaded as the conversion-specifier lead
character inside format strings (`sprintf('%d')`), but that is a library-level concern, not
lexical.

### 1.4 Line continuation
- `...` (three or more dots; canonically exactly three) at the end of a line continues the
  logical statement onto the next physical line. Text following `...` on the same line is
  treated as a comment and discarded.
- Continuation joins lines *before* tokenizing, so an operator or a partial expression may
  straddle the break.
- Continuation is **not** honored inside a single-quoted char literal or double-quoted
  string literal — a `...` inside a string is literal text. To break a long string, build
  it by concatenation across continued lines.
- Inside `[ ]`/`{ }`, a bare newline already acts as a row separator; use `...` to continue
  a *row* onto the next line without starting a new row.

---

## 2. Operators — Complete Precedence and Associativity

Precedence runs from **highest (evaluated first)** to **lowest**. Operators on the same
level have equal precedence and, unless noted, associate **left-to-right**.

| Level | Operators | Description | Assoc. |
|------:|-----------|-------------|--------|
| 1 (highest) | `( )` | Parentheses / grouping | n/a |
| 2 | `.'`  `.^`  `'`  `^` | Transpose (non-conjugate), element-wise power, complex-conjugate transpose, matrix power | left→right |
| 3 | `.^-` `.^+` `.^~`  `^-` `^+` `^~` | Power immediately followed by a **unary** operator on the exponent (`2^-1`, `A.^~b`) | **right→left** (see note) |
| 4 | `+` `-` `~` (unary) | Unary plus, unary minus, logical NOT | right→left |
| 5 | `.*` `./` `.\` `*` `/` `\` | Element-wise mul/div, matrix mul, matrix right/left division | left→right |
| 6 | `+` `-` (binary) | Addition, subtraction | left→right |
| 7 | `:` | Colon (range construction) | left→right |
| 8 | `<` `<=` `>` `>=` `==` `~=` | Relational | left→right |
| 9 | `&` | Element-wise logical AND | left→right |
| 10 | `\|` | Element-wise logical OR | left→right |
| 11 | `&&` | Short-circuit AND | left→right |
| 12 (lowest) | `\|\|` | Short-circuit OR | left→right |

### 2.1 Critical precedence subtleties (must implement exactly)

- **Unary binds *looser* than power.** `-2^2` parses as `-(2^2) = -4`, because level 2 (`^`)
  outranks level 4 (unary `-`). Likewise `2^-2` is `2^(-2)`, handled by the special level-3
  rule below.
- **The level-3 "power-then-unary" rule.** A power operator whose exponent is prefixed by a
  unary operator (`^-`, `^+`, `^~`, and their element-wise `.^` forms) is grouped as a unit
  that evaluates **from the second-rightmost operator toward the left** — i.e. these chains
  are **right-associative**. Concretely `2^-1^2` groups as `2^-(1^2)`. MathWorks explicitly
  advises using parentheses here because the grouping is unintuitive.
- **`&` outranks `|`.** Always. So `a | b & c` ≡ `a | (b & c)`. The same relationship holds
  for the short-circuit pair: `a || b && c` ≡ `a || (b && c)`. Element-wise operators (`&`,
  `|`) as a group outrank short-circuit operators (`&&`, `||`).
- **Colon is between arithmetic and relational.** `1:n+1` ≡ `1:(n+1)` (addition first), but
  `1:n < m` ≡ `(1:n) < m` (colon first). A frequent source of off-by-one surprises.
- **Transpose binds very tightly.** `A'*B` ≡ `(A')*(B)`; `-A'` ≡ `-(A')`.
- Within a level, left-to-right means `a/b/c` ≡ `(a/b)/c` and `a-b-c` ≡ `(a-b)-c`.

---

## 3. Literals

### 3.1 Numeric literals
- **Real decimals:** `3`, `3.14`, `.5`, `5.`, `1e3`, `1.2E-4`, `6.022e23`. Default class is
  **`double`** (IEEE-754 64-bit). There is no separate integer-literal class — `5` is a
  `double`, not an `int`.
- **No thousands separators**, no digit grouping.
- Leading-zero forms like `0x`/`0b` are reserved for hex/binary (below); a plain `0` is just
  double zero.

### 3.2 Imaginary literals
- A numeric literal with a trailing `i` or `j` (no operator, no space) is imaginary:
  `2i`, `3.5j`, `1e2i`. So `2i + 5` builds the complex value `5 + 2i`.
- `i` and `j` used *alone* (not as a suffix) are functions returning the imaginary unit
  `sqrt(-1)`; they can be shadowed by user variables named `i`/`j` (a classic loop-variable
  pitfall). The **suffix** form `2i` cannot be shadowed — it is lexical.

### 3.3 Hexadecimal and binary literals (R2019b+)
- **Hex:** prefix `0x` or `0X`, digits `0-9 A-F a-f` (digits case-insensitive). e.g. `0x2A` = 42.
- **Binary:** prefix `0b` or `0B`, digits `0`/`1`. e.g. `0b101010` = 42.
- **Default class:** the **smallest unsigned integer type** (`uint8`→`uint16`→`uint32`→`uint64`)
  that fits the value — *not* `double`. So `0x2A` is a `uint8`.
- **Type suffixes** override the class: `u8 u16 u32 u64` (unsigned) and `s8 s16 s32 s64`
  (signed). Examples: `0x2As32` → `int32(42)`; `0xFFs8` → `int8(-1)` (two's-complement
  reinterpretation within the chosen width); `0xFF000000001F123As64` → an `int64`.
- Stored as numbers, usable directly in arithmetic; **no quotes**.

### 3.4 Character and string literals
- **Char array** — single quotes: `'Hello'` is a 1×N `char` row vector (`N` code units).
  An empty `''` is a 0×0 char. Escape an embedded single quote by **doubling** it:
  `'it''s'` → `it's`. Char arrays are numeric under the hood (UTF-16 code units), so
  `double('A')` is `65` and char participates in arithmetic.
- **String** — double quotes (R2016b+): `"Hello"` is a 1×1 `string` **scalar** (an object
  wrapping the text), not a char vector. Escape an embedded double quote by **doubling**:
  `"she said ""hi"""` → `she said "hi"`.
- Neither literal form processes backslash escapes (`\n` is literal). Escape processing is a
  *function* concern (`sprintf`, `compose`, `double-quoted` in `fprintf` format args), not
  lexical.
- **Disambiguating `'`:** the lexer treats `'` as the **transpose** operator when it
  immediately follows a value-producing token (identifier, `)`, `]`, `}`, `.`field, another
  `'`, or a numeric literal); otherwise `'` opens a **char literal**. Thus `A'` is transpose,
  `('x')` is a string, and `A '` (with a space) is transpose of `A` followed by a char
  literal start — spacing matters here.

---

## 4. Operator Semantics: Scalars vs. Matrices

### 4.1 Element-wise vs. matrix operators
| Element-wise | Matrix (linear-algebra) |
|--------------|-------------------------|
| `.*` element product | `*` matrix product (inner dims must agree) |
| `./` element right-divide | `/` `mrdivide`: `A/B` solves `x*B = A` |
| `.\` element left-divide (`a.\b` = `b./a`) | `\` `mldivide`: `A\B` solves `A*x = B` (least-squares/backslash solve) |
| `.^` element power | `^` matrix power (`A^2` = `A*A`; requires square `A`, or scalar exponent) |
| `.'` transpose (no conjugation) | `'` conjugate transpose |

- `+` and `-` are always element-wise (there is no separate matrix add).
- **Scalar–array:** when one operand is a scalar, every operator applies the scalar to each
  element (`2 * A`, `A .^ 2`, `A + 1`). For a scalar operand `*` and `.*` coincide, as do
  `/`÷`./`, and `^`÷`.^` behave the same only when the *base or exponent* is scalar in the
  intended element sense — note `A^2` (matrix square) ≠ `A.^2` (element squares) for non-scalar `A`.

### 4.2 The two transpose operators (must not conflate)
- `.'` — **non-conjugate transpose** (`transpose`): swaps dimensions only.
- `'` — **complex-conjugate transpose** (`ctranspose`): swaps dimensions **and** conjugates
  each element. For real arrays the two are identical; for complex arrays they differ. A
  very common porting bug is using `'` on complex data expecting a plain transpose.

### 4.3 Implicit expansion (broadcasting, R2016b+)
Most binary element-wise operators (and relational/logical ones) accept operands of
**compatible sizes**, defined as: *for every dimension, the two sizes are equal or one of
them is 1*. The size-1 dimensions are virtually replicated. Examples:
- column `(m×1)` op row `(1×n)` → `m×n`.
- `4×2` op `4×1` → `4×2`.
- Incompatible: `3×2` op `4×2` (neither row count is 1) → error (see §7).
Scalar-with-anything is the degenerate case of this rule.

### 4.4 Colon operator `:`
- `j:k` → row vector `[j, j+1, …]` with `m = floor(k-j)` steps; empty (`1×0`) if `k < j`.
- `j:i:k` → step `i`; `m = floor((k-j)/i)`; empty if the step points away from `k`
  (e.g. `1:-1:5` is empty). Non-integer steps are subject to floating-point endpoint
  inclusion (the endpoint `k` may or may not appear depending on rounding), so ranges like
  `0:0.1:1` should be treated carefully by an implementer for endpoint fidelity.
- As an index, a lone `:` means "all elements of this dimension" (`A(:,2)`, `A(:)` linearizes
  column-major).

---

## 5. Relational and Logical Semantics

### 5.1 Relational operators (`< <= > >= == ~=`)
- Perform **element-wise** comparison and return a **`logical` array** of the compatible
  (broadcast) size, `1`=true / `0`=false.
- **Complex operands:** `<`, `<=`, `>`, `>=` compare **only the real parts**; `==` and `~=`
  compare **both** real and imaginary parts.
- **`NaN`:** unordered — `NaN == NaN` is `false`, `NaN ~= x` is `true` for all `x`, and every
  ordering comparison against `NaN` is `false`. (`isnan` is the way to test.)
- Char/string/numeric mixing follows conversion rules (char compares by code unit).

### 5.2 Element-wise logical (`& | ~`)
- `&` (AND), `|` (OR), `~` (NOT) operate **element-wise**, return a `logical` array of the
  broadcast size, and **evaluate both operands fully** (no short-circuiting). Operands are
  converted to logical (nonzero → true, zero → false).
- `~` is unary NOT (also spelled `~` in `~=`). `~x` and `not(x)` are equivalent.

### 5.3 Short-circuit logical (`&& ||`)
- Require operands that reduce to a **scalar logical** value. `A && B` does **not** evaluate
  `B` when `A` is false; `A || B` does **not** evaluate `B` when `A` is true.
- Return a scalar `logical`.
- **A non-scalar operand is an error**, not a silent `any`/`all`. Use `any(...)`/`all(...)`
  to collapse a vector before a short-circuit test.
- Intended for guard conditions in `if`/`while`, e.g. `b ~= 0 && a/b > k` (the division is
  skipped when `b == 0`), or `isfield(S,'f') && ~isempty(S.f)`.
- Precedence: `&&`/`||` sit **below** `&`/`|`, which sit below relational — so
  `a < b && c > d` parses as `(a<b) && (c>d)` with no parentheses needed.

---

## 6. Command Syntax vs. Function Syntax

MATLAB accepts two call forms with identical semantics but different argument passing:

- **Function syntax:** `func(arg1, arg2)` and `[out1,out2] = func(args)`. Arguments are
  ordinary expressions — data, variables, or subexpressions — evaluated and passed by value.
- **Command syntax:** `func arg1 arg2` — arguments separated by **spaces**, no parentheses,
  no commas. **Every argument is passed as a char vector** (as if single-quoted), and **no
  output can be captured** (result goes to `ans` or is discarded). Example: `hold on` ≡
  `hold('on')`; `format long` ≡ `format('long')`; `disp hello world` ≡ `disp('hello','world')`
  (two args) — to pass one string with a space, use quotes or function syntax.

### 6.1 Disambiguation rules (how the parser decides)
After an identifier at statement start, MATLAB inspects following characters:
1. `=` (not `==`) → **assignment** statement.
2. `(` immediately (no space) → **function-call / indexing** (function syntax).
3. A **space then a token that is not a balanced binary operator** → **command syntax**.
4. An operator with **spaces on both sides** *or* **no spaces on either side** → treated as
   an **expression** (function/variable operation), not a command.

The decisive, notorious case is **a space on only one side of an operator**:
- `x = ls ./d` and `x = ls./d` → division (`ls` treated as a variable/expression).
- `ls ./d` (space before `./`, none after) → **command syntax**: call `ls` with the literal
  argument `'./d'`.
- Analogously `y = x -1` at the command line: because `-` has a space before and not after,
  MATLAB *may* read this as command syntax `y('x', '-1')` if `y` is a command-eligible name;
  `x - 1` and `x-1` are arithmetic. The safe rule: **balanced spacing keeps it arithmetic.**

### 6.2 Command-syntax limitations (why an implementer must restrict it)
- Cannot pass numeric or non-char data: `isnumeric 500` calls `isnumeric('500')` → `false`.
- Cannot pass a variable's *value*: `open filename` passes the literal `'filename'`, not the
  contents of a variable `filename`; use `open(filename)`.
- Cannot capture outputs into named variables.
- Only usable where all intended arguments are literal text without embedded spaces (unless
  quoted). It is fundamentally sugar for "call this function with these string literals."

---

## 7. Statements, Terminators, Display, and `ans`

### 7.1 Terminators and separators
- `;` ends a statement **and suppresses** automatic display of its result.
- `,` (and newline) ends a statement **without** suppressing display. Multiple statements may
  share a line separated by `,` or `;` (`a = 1, b = 2;`).
- A statement with **no** trailing `;` (or ending in `,`/newline) **auto-displays** its
  result.
- Inside `[ ]`/`{ }`, `;` and newline mean *new row*, and `,`/space mean *new column* — a
  different role from statement termination.

### 7.2 Automatic display
When an unsuppressed statement produces a value:
- **Assignment** `x = expr` echoes `x = <value>`.
- **Bare expression** `expr` (no assignment) assigns the value to the special variable
  **`ans`** and echoes `ans = <value>`.
- The exact formatting of `<value>` depends on the current `format` setting (`short`,
  `long`, etc.) and the class/size of the result; large arrays print with size headers.
- `;` suppresses this echo but does **not** suppress explicit output functions such as
  `disp`, `fprintf`, or `warning`.

### 7.3 The `ans` variable
- `ans` ("answer") is an automatic workspace variable holding the result of the most recent
  **unassigned, unsuppressed** expression (or a function called without capturing its output).
- It is overwritten on each such statement and can be referenced like any variable
  (`ans + 1`). Relying on it in scripts is discouraged but supported.

---

## 8. Common Error / Warning Messages (user-facing text, for compatibility)

These are observable, public user-facing strings. Reproducing the *message text and
identifier* is important for drop-in compatibility (tests, `try/catch` on `ME.identifier`).
Text has drifted slightly across releases; both older and current forms are noted.

| Situation | Identifier | Message text (paraphrased/observed) |
|-----------|-----------|--------------------------------------|
| Reference to unknown name | `MATLAB:UndefinedFunction` | `Unrecognized function or variable 'NAME'.` (R2018a+). Earlier: `Undefined function or variable 'NAME'.` For a bad method/arg combo: `Undefined function 'NAME' for input arguments of type 'CLASS'.` |
| Index past array bounds | `MATLAB:badsubscript` (or `MATLAB:index...`) | `Index exceeds the number of array elements. Index must not exceed N.` (R2018b+). Earlier: `Index exceeds matrix dimensions.` |
| Non-positive / non-integer index | `MATLAB:badsubscript` | `Array indices must be positive integers or logical values.` |
| Element-wise size mismatch | `MATLAB:sizeDimensionsMustMatch` | `Arrays have incompatible sizes for this operation.` (R2016b+). Earlier: `Matrix dimensions must agree.` |
| Matrix multiply inner-dim mismatch | `MATLAB:innerdim` (or `MATLAB:dimagree`) | `Incorrect dimensions for matrix multiplication. Check that the number of columns in the first matrix matches the number of rows in the second matrix. To operate on each element ... use '.*' instead of '*'.` (current, with the `.*` hint) |
| Vertical concatenation mismatch | `MATLAB:catenate:dimensionMismatch` | `Dimensions of arrays being concatenated are not consistent.` (older: `CAT arguments dimensions are not consistent.`) |
| Too many inputs to a function | `MATLAB:TooManyInputs` | `Too many input arguments.` |
| Too many outputs requested | `MATLAB:TooManyOutputs` / `maxlhs` | `Too many output arguments.` |
| Unbalanced / parse error | `MATLAB:...` (varies) | e.g. `Invalid expression. Check for missing multiplication operator, missing or unbalanced delimiters, or other syntax error.` |
| Non-scalar operand to `&&`/`||` | `MATLAB:nonLogicalConditional` (context-dependent) | `Operands to the logical AND (&&) and OR (||) operators must be convertible to logical scalar values.` |

Notes for the implementer:
- Match on **`identifier`** for robustness; the human text changes across versions but the
  identifier is stable and is what `catch ME`/`error(id,...)` code keys off.
- Preserve the `'NAME'` interpolation and the trailing period; tooling and users pattern-match
  these strings.

---

## 9. Implementer Checklist (the subtle, non-obvious parts)

1. **Bracket whitespace unary rule** (`[1 -1]` vs `[1 - 1]`) — a lexer-mode, not a parser
   afterthought.
2. **Unary is *lower* precedence than power** (`-2^2 = -4`), and the `^-`/`.^-` chains are
   **right-associative** (level 3).
3. **`&` > `|`** and **`&`/`|` > `&&`/`||`** in precedence; colon sits between `+/-` and
   relational.
4. **Two transposes** (`'` conjugates, `.'` does not) and **`'` vs char-literal**
   disambiguation depends on the preceding token.
5. **Hex/binary literals default to the smallest unsigned integer type**, not double, and
   carry `u8..u64`/`s8..s64` suffixes (R2019b+).
6. **`string` (double-quote) vs `char` (single-quote)** are different classes with different
   sizes for "the same" text; escaping is quote-doubling in both.
7. **Short-circuit operators require scalar operands** and error otherwise — they are not
   silent `any`/`all`.
8. **Complex relational**: ordering compares real parts only; `==`/`~=` compare both parts;
   `NaN` is unordered and never equal to itself.
9. **Command syntax** passes all args as char, captures no outputs, and is chosen by a
   space-sensitive rule (one-sided operator spacing is the trap).
10. **`ans`** captures every unsuppressed, unassigned expression; `;` suppresses display but
    not side-effecting output functions.

---

## 10. Sources (public MathWorks documentation)

- Operator Precedence — https://www.mathworks.com/help/matlab/matlab_prog/operator-precedence.html
- MATLAB Operators and Special Characters — https://www.mathworks.com/help/matlab/matlab_prog/matlab-operators-and-special-characters.html
- Array Comparison with Relational Operators — https://www.mathworks.com/help/matlab/matlab_prog/array-comparison-with-relational-operators.html
- Relational Operations (overview) — https://www.mathworks.com/help/matlab/relational-operators.html
- Short-Circuit AND (`&&`) — https://www.mathworks.com/help/matlab/ref/shortcircuitand.html
- Short-Circuit OR (`||`) — https://www.mathworks.com/help/matlab/ref/shortcircuitor.html
- Compatible Array Sizes for Basic Operations (implicit expansion) — https://www.mathworks.com/help/matlab/matlab_prog/compatible-array-sizes-for-basic-operations.html
- Colon operator — https://www.mathworks.com/help/matlab/ref/double.colon.html
- Add Comments to Code — https://www.mathworks.com/help/matlab/matlab_prog/comments.html
- Block comment `%{ %}` — https://www.mathworks.com/help/matlab/ref/blockcomment.html
- Hexadecimal and Binary Values — https://www.mathworks.com/help/matlab/matlab_prog/specify-hexadecimal-and-binary-numbers.html
- Represent Text with Character and String Arrays — https://www.mathworks.com/help/matlab/matlab_prog/represent-text-with-character-and-string-arrays.html
- Create Complex Numbers / imaginary unit `i` — https://www.mathworks.com/help/matlab/matlab_prog/complex-numbers.html , https://www.mathworks.com/help/matlab/ref/i.html
- Choose Command Syntax or Function Syntax — https://www.mathworks.com/help/matlab/matlab_prog/command-vs-function-syntax.html
</content>
</invoke>
