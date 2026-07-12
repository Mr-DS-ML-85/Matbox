# MATBOX MATLAB Language Spec — 04: Control Flow, Functions, Scoping, Error Handling

> Clean-room implementation reference. Documents *publicly documented behavior* of the
> MATLAB language in our own words, derived from public MathWorks documentation
> (mathworks.com/help) and other public references. No MathWorks source code is used
> or reproduced. URLs are cited per section for traceability.

---

## 0. The Truthiness Rule (foundational)

Every conditional context (`if`, `elseif`, `while`, short-circuit operands) evaluates an
expression to a **single boolean** using one uniform rule:

> An expression is **true** when its result is **nonempty AND contains only nonzero
> elements** (logical or real numeric). Otherwise it is **false**.

Consequences the implementation must honor:
- A scalar `0` / `false` → false; any nonzero scalar → true.
- An **array** condition (e.g. `A > 0`) is true **only if *every* element is nonzero**.
  This is an implicit `all(cond(:))` over the whole array. To test "any element", the
  programmer must wrap with `any(...)`.
- An **empty** array (`[]`, `''`, `0x0`, `1x0`, ...) is **false**, even though it contains
  no zero elements. Emptiness dominates.
- `NaN` is nonzero → true. (Only exact zero is falsey.)
- Complex numbers: MATLAB uses the **real part only** for truth testing in conditionals
  (documented as "logical or real numeric"); implement by testing the real component's
  nonzero-ness. (Historically MATLAB warns/considers only real part.)

Refs:
- https://www.mathworks.com/help/matlab/ref/if.html
- https://www.mathworks.com/help/matlab/ref/while.html

---

## 1. Control Flow

### 1.1 `if` / `elseif` / `else` / `end`

```matlab
if expr
    stmts
elseif expr2      % zero or more elseif clauses
    stmts
else              % optional, at most one
    stmts
end
```
- Clauses evaluated top to bottom; the **first** true clause runs, others are skipped.
- `elseif` is one token (no space). `else if` would open a *nested* `if` needing its own `end`.
- Condition uses the truthiness rule (§0).
- Prefer `&&`/`||` (short-circuit, scalar) inside conditions over `&`/`|` (elementwise).

Ref: https://www.mathworks.com/help/matlab/ref/if.html

### 1.2 `for` — and the column-iteration rule (critical)

```matlab
for index = values
    stmts
end
```

**How `values` is consumed — MATLAB iterates over COLUMNS:**
- General form `for index = A`: on each iteration, `index` is assigned **the next COLUMN
  of `A`** as a column vector. The loop runs `size(A,2)` times (once per column).
  - So `for i = [1 2 3]` (a 1x3 row) → 3 iterations, `i` = 1, then 2, then 3 (each a 1x1 column).
  - `for i = [1;2;3]` (a 3x1 column) → **1 iteration**, with `i = [1;2;3]` the whole column.
    To loop element-by-element over a column vector, **transpose it first**: `for i = v'`.
  - `for c = magic(3)` → 3 iterations, each `c` is a 3x1 column of the magic square.
- Works with **any type**: numeric/char matrices (column vectors), **cell arrays** (each
  iteration `index` is a 1x1 cell = the next column of cells), and **struct arrays**.
- Range forms are just special cases that build a row vector:
  - `for k = initVal:endVal` → increment by 1 until `k > endVal` (empty range → 0 iterations,
    e.g. `5:1`).
  - `for k = initVal:step:endVal` → increment by `step`; if `step < 0`, decrements until
    `k < endVal`. Non-integer steps allowed (floating accumulation applies).

**Index-variable rules:**
- Assigning to `index` inside the body has **no effect on iteration**: "The for statement
  overrides any changes made to index within the loop." (The value is reset to the next
  column each iteration.)
- After the loop completes normally, `index` retains its **last** assigned value (it is a
  normal variable in the function workspace, not scoped to the loop).
- The `values` expression is evaluated **once** at loop entry; later mutation of the source
  variable does not change the iteration set.

Ref: https://www.mathworks.com/help/matlab/ref/for.html

### 1.3 `while`

```matlab
while expr
    stmts
end
```
- Repeats while `expr` is true (truthiness rule, §0). Matrix condition ⇒ all elements
  must be nonzero; wrap with `any(...)` to loop while *any* element is true.
- `Ctrl+C` interrupts a runaway loop (interactive concern, not language semantics).

Ref: https://www.mathworks.com/help/matlab/ref/while.html

### 1.4 `switch` / `case` / `otherwise` / `end`

```matlab
switch switch_expr
    case case_expr           % scalar, char/string, OR cell array of alternatives
        stmts
    case {val1, val2, val3}  % matches if ANY element equals switch_expr
        stmts
    otherwise                % optional; runs if no case matched
        stmts
end
```

Matching semantics (implement exactly):
- Cases tested **top to bottom**; the **first** matching case runs.
- **No fall-through** (unlike C): after a matching case body runs, control jumps to after
  `end`. No `break` is needed or used inside `case`.
- Match rule depends on types:
  - **Numbers / logicals**: equality `case_expr == switch_expr` (must yield scalar true).
  - **Char vectors / strings**: comparison via `strcmp` semantics (whole-string equality,
    not elementwise).
  - **Cell-array case** `{a,b,c}`: matches if **at least one element** of the cell equals
    `switch_expr` (each compared by the number/char rule above). This is the idiomatic
    "match any of these" construct.
  - **Objects**: use the class's `eq` overload (must return a logical scalar).
- `switch_expr` should evaluate to a **scalar** value or a **char vector/string** (not a
  general array).
- `otherwise` is optional and runs only when nothing matched.

Ref: https://www.mathworks.com/help/matlab/ref/switch.html

### 1.5 `break` and `continue`

- `break`: immediately exit the **innermost** enclosing `for`/`while` loop. Does not exit
  the function. Not valid outside a loop.
- `continue`: skip the rest of the current iteration and re-enter the loop (re-evaluating
  the `while` condition / advancing to the next `for` column).
- In nested loops, both affect only the innermost loop.

Refs: https://www.mathworks.com/help/matlab/ref/break.html ,
https://www.mathworks.com/help/matlab/ref/continue.html

### 1.6 `return`

- `return` **exits the entire currently executing function or script**, returning control
  to the caller (or command prompt). It is *not* a loop-break: from inside nested loops /
  `if` / `switch`, it unwinds all of them and leaves the function.
- Output arguments take whatever values they currently hold at the point of `return`
  (unassigned declared outputs will error if the caller requested them — see §4).

Ref: https://www.mathworks.com/help/matlab/ref/return.html

### 1.7 `parfor` (documented behavior; requires Parallel Computing Toolbox)

```matlab
parfor loopVar = initVal:endVal
    stmts
end
```
- Executes iterations **in parallel across workers, in nondeterministic order**.
- **Iterations must be independent**: no iteration may depend on results of another. The
  implementation/programmer contract is order-independence.
- Loop range must be **consecutive integers** (increasing; decreasing allowed only in very
  recent releases).
- **Cannot** `break` or otherwise exit early; the client is blocked until all iterations
  finish. `return` inside `parfor` is likewise disallowed.
- **Cannot** nest `parfor` directly inside another `parfor`.
- Variable classification MATLAB performs statically:
  - **Loop variable** — the index; cannot be modified in the body.
  - **Sliced variable** — array indexed by `loopVar` (e.g. `A(i)`); each worker gets its slice.
  - **Broadcast variable** — read-only value defined before the loop, copied to all workers.
  - **Reduction variable** — accumulated with an associative/commutative op (`+`, `*`,
    `[ ]` concat, `min`, `max`, ...) in an order-independent way.
  - **Temporary variable** — created fresh inside each iteration, not returned.
- If no pool exists, one is created automatically (unless disabled in settings).

Ref: https://www.mathworks.com/help/parallel-computing/parfor.html

---

## 2. Function Definitions & File Types

### 2.1 Function file vs. script file

- **Function file**: the file's first executable construct is a `function` definition. The
  **main (primary) function** shares the file's name and is the entry point callable from
  outside. Its name for calling purposes is the *filename*, not the declared name if they
  differ (filename wins).
- **Script file**: contains commands executed in the **base/caller workspace** with no
  `function` header at the top.
- **Local functions in scripts**: allowed (R2016b+). Historically they had to appear at
  the **end** of the script file; since R2024a they may appear anywhere in the script — but
  **never inside** a control block (`if`/`for`/`while`/`switch`/`try`). Same rule applies
  to function files: a local function cannot be defined inside a control block.

### 2.2 Function definition syntax

```matlab
function [out1, out2] = name(in1, in2)
    ...
end          % 'end' optional in function files UNLESS the file uses nested functions,
             % local functions in a script, or any function in the file needs 'end'
```
- The `[...]` on outputs is optional for a single output: `function y = f(x)`.
- No outputs: `function name(x)` (or `function [] = name(x)`).
- **`end` termination**: optional for classic function files with only local functions and
  no nesting. **Required for ALL functions in a file** as soon as the file contains any
  **nested function**, or when the functions live in a **script**. Best practice: always
  terminate with `end` for consistency.
- **`endfunction`**: This is an **Octave** keyword, **not** MATLAB. MATLAB uses `end`.
  A MATBOX implementation aiming for Octave compatibility may accept `endfunction` as an
  alias, but it is not documented MATLAB behavior; keep it behind a compatibility flag.

### 2.3 Function types

| Type | Where defined | Visibility | Key trait |
|------|--------------|-----------|-----------|
| **Main/primary** | top of a function file | callable by filename from anywhere on path | file entry point |
| **Local** | after the main function in same file (or in a script) | only within that file | own workspace; ordering among locals is free |
| **Nested** | *inside* another function's body | within parent + sibling nested fns | **shares parent workspace** (§5.5) |
| **Anonymous** | any expression, `@(args) expr` | wherever the handle is passed | captures workspace by value (§2.4) |
| **Private** | in a `private/` subfolder | only functions in the **parent folder** (and its private fns) | folder-scoped visibility |

- **Private functions** live in a subfolder literally named `private`. They are visible only
  to functions defined in the folder immediately **above** `private/` (and to other private
  functions there). They take **precedence** over path functions of the same name for those
  callers — useful for overriding without polluting the global namespace.

### 2.4 Anonymous functions

```matlab
sqr = @(x) x.^2;             % single expression, returns one value
parabola = @(x) a*x.^2 + b*x + c;   % a,b,c captured NOW, by value
```
- Syntax `@(arglist) expression` — exactly **one** expression, no statements, no local `end`.
- **Capture semantics**: variables referenced in the body that are **not** parameters are
  captured **by value at handle-creation time** (a frozen snapshot). Later changes to those
  outer variables — or `clear`ing them — do **not** affect the handle.
- An anonymous function may reference **only** its parameters and captured variables. If its
  body references any name that is neither a parameter nor a capturable workspace variable
  (nor a valid function), MATLAB errors when the handle is **invoked**.
- Handles are **scalars**; to hold many, use a cell array: `f = {@sin, @(x)x+1}; f{2}(3)`.
- Returns a single output; multiple outputs only via tricks like `deal`.

Refs:
- https://www.mathworks.com/help/matlab/matlab_prog/types-of-functions.html
- https://www.mathworks.com/help/matlab/matlab_prog/anonymous-functions.html
- https://www.mathworks.com/help/matlab/matlab_prog/nested-functions.html

---

## 3. Argument Handling

### 3.1 `nargin` / `nargout` (inside a function)

- `nargin` (no args, inside a function) → number of inputs **actually supplied** by the caller.
- `nargout` (no args, inside a function) → number of outputs **the caller requested**
  (i.e. LHS count). Drives conditional/lazy output computation:
  ```matlab
  function [a, b] = f(x)
      a = cheap(x);
      if nargout > 1
          b = expensive(x);   % only compute when caller asked for 2 outputs
      end
  end
  ```
  - `f(x)` or `a = f(x)` → `nargout == 1`.
  - `[a,b] = f(x)` → `nargout == 2`.
  - `f(x)` called as a bare statement → `nargout == 0` (but one output may still print/ans).
- Query forms (given a name/handle, not the running call):
  - `nargin('fun')` / `nargin(@fun)` → declared input count. If the function declares
    `varargin`, returns the **negative** of (number of declared inputs).
  - `nargout('fun')` → declared output count; **negative** if it declares `varargout`
    (e.g. `[y,z,varargout]` → `-3`).

Refs: https://www.mathworks.com/help/matlab/ref/nargin.html ,
https://www.mathworks.com/help/matlab/ref/nargout.html

### 3.2 `varargin` / `varargout`

- `varargin` — **must be the last input parameter**. Collects all trailing actual arguments
  into a **cell array**, one arg per cell (`varargin{1}`, `varargin{2}`, ...). `nargin`
  counts fixed args + all `varargin` cells; `numel(varargin)` = number of extra args.
- `varargout` — **must be the last output parameter**. A cell array the function fills;
  MATLAB returns `varargout{1}`, `varargout{2}`, ... as outputs. The function typically
  sizes it using `nargout`.
- Expand a cell into a comma-separated argument list with `varargin{:}` (e.g. to forward
  args: `plot(varargin{:})`).
- `varargin`/`varargout` can be the sole parameter or follow fixed parameters
  (`function f(a, b, varargin)`).

### 3.3 `inputname`

- `inputname(k)` (inside a function) → the **workspace variable name** of the k-th input
  argument as it appeared in the caller. Returns `''` (empty char) if that argument was an
  **expression, literal, or otherwise not a simple variable**. Useful for diagnostics/error
  messages. Also `inputname(argHandle)` variants in newer releases.

Refs: https://www.mathworks.com/help/matlab/matlab_prog/support-variable-number-of-inputs.html ,
https://www.mathworks.com/help/matlab/ref/inputname.html

### 3.4 Default-argument idioms (pre-`arguments` block)

MATLAB has no built-in default-value syntax in the classic signature; idioms:
```matlab
function y = f(x, tol)
    if nargin < 2 || isempty(tol)
        tol = 1e-6;            % default
    end
    ...
end
```
- Common patterns: `if ~exist('tol','var'), tol = ...; end`, or the `nargin`-switch ladder.
- The `[]`-means-default convention (`isempty` check) lets callers pass `[]` to skip a
  middle positional argument.

### 3.5 The `arguments` block (R2019b+) — validation & defaults

Placed **at the very top of the function body, before any executable code** (only comments
may precede). Declares inputs (and, R2022b+, outputs) with size/class/validation/defaults.

```matlab
function out = f(a, b, c, options)
    arguments
        a (1,1) double {mustBePositive}          % required, scalar positive double
        b double {mustBeNumeric} = 0             % optional, default 0
        c (1,:) char = 'x'                        % optional row char, default 'x'
        options.Tolerance (1,1) double {mustBeNonnegative} = 1e-6   % name-value
        options.Method (1,1) string = "auto"
    end
    ...
end
```

Declaration form (each line):
```
argName (dimSpec) ClassName {validatorFcn, ...} = defaultValue
```
Every part except `argName` is optional. Semantics:
- **`(dimSpec)`** — required size, e.g. `(1,1)` scalar, `(1,:)` row of any length, `(:,:)`
  any 2-D, `(3,4)` exact. A mismatch errors. `:` means "any length in that dimension".
- **`ClassName`** — required class. The input must be that class **or convertible to it**;
  if convertible (e.g. `single`→`double`, numeric→`char`), MATLAB **converts** the argument
  (class validation may mutate the value, unlike validators).
- **`{validators}`** — comma-separated validation functions (`mustBePositive`,
  `mustBeNonnegative`, `mustBeNumeric`, `mustBeMember`, `mustBeInteger`, `mustBeText`,
  `mustBeFinite`, `mustBeReal`, ... plus user functions). They **error** on failure and
  **do not modify** the value. Applied after size/class.
- **`= defaultValue`** — makes the argument **optional**. All optional (defaulted) positional
  arguments must come **after** all required ones (in both signature and block order). The
  default expression is only evaluated when the argument is omitted.

Order of processing per argument: **size → class (may convert) → validators**.

**Name-value arguments**: declared via a struct-style name `options.Name`. Called as
`f(a, Tolerance=1e-8, Method="fast")` or classic `f(a,'Tolerance',1e-8)`. The `options`
struct name is arbitrary; each `options.Field` becomes one name-value parameter. Name-value
declarations must come **after** positional ones.

**Repeating arguments** (`arguments (Repeating)`): a group of positional args that may repeat
in sets; MATLAB collects each declared name into a **cell array** across the repetitions.
Only one Repeating block per function; it precedes any name-value block.
```matlab
arguments (Repeating)
    x (1,:) double
    y (1,:) double
end               % call f(x1,y1, x2,y2, x3,y3) → x = {x1,x2,x3}, y = {y1,y2,y3}
```

**Output arguments** (`arguments (Output)`, R2022b+): may impose size/class/validators on
outputs but **cannot** have default values. `arguments (Output, Repeating)` also exists.

**Restrictions**: `arguments` blocks are **not** supported in nested functions or in abstract
methods; a function using an `arguments` block cannot also declare defaults the old way for
the same args. Multiple blocks need explicit labels (`(Input)`, `(Output)`, `(Repeating)`).

Ref: https://www.mathworks.com/help/matlab/ref/arguments.html

---

## 4. Multiple Outputs & `nargout`-Dependent Behavior

- A function returns as many values as the caller binds on the LHS, up to its declared
  outputs (or unbounded with `varargout`).
- Outputs are assigned **positionally** to LHS targets: `[a,b,c] = f(...)`.
- A **declared output that the caller requests but the function never assigned** raises an
  error ("Output argument 'b' ... not assigned"). Only outputs *up to* `nargout` need to be
  assigned; trailing unrequested outputs may remain unset.
- **Ignoring outputs with `~`**: `[~, idx] = max(v);` discards the first output. `~` is a
  placeholder that binds nothing (cannot be read). Multiple `~` allowed: `[~,~,c] = f();`.
- `nargout` lets a function **skip computing** unrequested outputs (see §3.1 example) — this
  is observable behavior callers rely on (e.g. `size` returns a vector for 1 output vs.
  separate dims for N outputs; `find` behaves differently by `nargout`). MATBOX must expose
  `nargout` to the executing frame accordingly.

Refs: https://www.mathworks.com/help/matlab/ref/nargout.html ,
https://www.mathworks.com/help/matlab/matlab_prog/ignore-function-outputs.html

---

## 5. Scoping & Workspaces

### 5.1 Base workspace vs. function workspaces
- The **base workspace** holds variables created at the command line / by scripts run from
  the command line. Scripts execute in the **caller's** workspace (base, if called from base).
- Each **function call** gets a **fresh, isolated workspace**. Function-local variables are
  invisible to the caller and vice versa. They vanish when the function returns.
- Cross-workspace access is only via `assignin`/`evalin` (`'base'`/`'caller'`) — explicit,
  not implicit.

### 5.2 Pass-by-value + copy-on-write
- Arguments are **passed by value**: the callee gets its own logical copy; mutating a
  parameter does **not** affect the caller's variable.
- Implementation optimization (documented behavior): MATLAB uses **copy-on-write** — the copy
  is deferred/shared until the callee **modifies** the value, at which point a real copy is
  made. MATBOX should implement COW so that passing large arrays read-only is cheap, but
  semantics remain pure value semantics.
- Handle classes and function handles are references (the handle is copied by value, but it
  points to shared underlying state) — the exception to "modifying a param can't affect caller".

### 5.3 `global`
```matlab
global G
```
- Creates/links `G` in a **separate global namespace**. **Every** workspace (each function,
  and the command line) that wants access must issue its own `global G` declaration; only
  then does its local name `G` bind to the shared storage.
- Changes are visible to **all** functions that declared it global. First-ever declaration
  initializes it to `[]` (0x0).
- The `global` declaration must precede first use of the name in that function.
- Documented guidance: globals are error-prone and inefficient; prefer passing arguments.

Ref: https://www.mathworks.com/help/matlab/ref/global.html

### 5.4 `persistent`
```matlab
persistent P
```
- A variable **local to the declaring function** whose value **persists across calls** (retained
  in memory between invocations). Other functions and the command line cannot see or change it.
- On the **first** encounter of the `persistent` statement it is initialized to `[]` (empty).
  Idiom: `if isempty(P), P = init(); end` to run one-time setup.
- The `persistent` declaration must **precede any other reference** to the name (including its
  use as an input/output argument — a name cannot be both an argument and persistent, nor be
  assigned before being declared persistent).
- Cleared when the function is **cleared or modified** (`clear functionname`, editing the
  file). `mlock` prevents that clearing; `munlock` re-enables it.

Ref: https://www.mathworks.com/help/matlab/ref/persistent.html

### 5.5 Nested-function scoping (critical & distinctive)

A **nested function** is defined entirely inside a parent function's body. Its scoping differs
fundamentally from local functions:

- **Shared workspace**: a nested function **can read and modify variables of its parent**
  (enclosing) function, and vice versa — *without* passing them as arguments. They literally
  share the same variable storage for any name used in both.
- **Which variables are shared** is determined **statically** by usage:
  - A variable used by **both** the parent and a nested function (or by two sibling nested
    functions and the parent) is **shared** across that scope.
  - A variable used **only inside** one nested function stays **local** to that nested function.
  - This means the set of shared variables is fixed at parse time by textual reference — the
    implementation must do this analysis, not create variables on the fly.
- **Requirements / restrictions**:
  - If a file contains **any** nested function, **every** function in that file must be
    terminated with `end`.
  - Nested functions **cannot** be defined inside control statements (`if`/`for`/`while`/
    `switch`/`try`).
  - All variables in functions containing nested functions must be **explicitly defined**;
    you cannot implicitly create a variable via, e.g., `load`/`eval` into that workspace
    (the parser must know all names up front).
  - You must call a nested function **by name** or through an `@` handle to it — **not** via
    `feval('nestedName', ...)` or `str2func` (they can't resolve nested names).
- **Handles to nested functions capture the shared workspace**: `h = @nestedFn;` stores not
  just the function identity but a **reference to the live variables** it shares with the
  parent (the values of variables the nested function references). Unlike an anonymous
  function's frozen by-value snapshot, a nested-function handle sees **subsequent updates** to
  those shared variables (they remain live as long as the handle exists). This is the standard
  mechanism for stateful callbacks / closures in MATLAB.

Contrast with **local functions**: local (subfunction) workspaces are **fully isolated** — no
shared variables with the main or other local functions; communication is only via arguments
and return values.

Refs: https://www.mathworks.com/help/matlab/matlab_prog/nested-functions.html ,
https://www.mathworks.com/help/matlab/matlab_prog/sharing-variables-between-parent-and-nested-functions.html

---

## 6. Error Handling

### 6.1 Error identifier format `component:mnemonic`
- Form: one or more `component` fields and a final `mnemonic`, colon-separated:
  `'MyPkg:myFunc:badInput'`, `'MATLAB:narginchk:notEnoughInputs'`.
- Each field must **begin with a letter**; remaining chars may be alphanumeric or underscore;
  **no whitespace anywhere**. A single unqualified word is *not* a valid identifier (must
  contain at least one colon to be treated as an ID).

### 6.2 `error()`
```matlab
error(msg)                          % literal message (no escape processing if single arg)
error(formatMsg, A1, A2, ...)       % sprintf-style: %s %d \n processed
error(errID, formatMsg, A1, ...)    % identifier + formatted message
error(errorStruct)                  % struct with fields message/identifier/stack
error(msgID, ...)                   % also error(exception) with an MException
error(correction, msg, ...)         % with a suggested-fix correction object
```
- Throws immediately, unwinds to the nearest enclosing `try` (or aborts to prompt).
- **Escape/format processing only happens when there are ≥2 input args.** With a single
  string argument, `\n`, `%s`, `%d`, `\t` are taken **literally** (not interpreted). To force
  a newline with one logical message, use `error('MyID:x','line1\nline2')` or `sprintf`.
- `errorStruct` must have at least one of `message` / `identifier` / `stack`.
- Edge case: **if all inputs are empty, `error` does nothing** (no error thrown).

Ref: https://www.mathworks.com/help/matlab/ref/error.html

### 6.3 `MException` (the exception object)
```matlab
ME = MException(errID, formatMsg, A1, ...);
throw(ME);            % or ME.throw
```
- Constructor: identifier (must be a valid ID), message (sprintf-formatted with trailing args).
- Read-only properties:
  - `identifier` — the error ID string.
  - `message` — formatted message text.
  - `stack` — struct array (`file`, `name`, `line`) of the call stack where thrown.
  - `cause` — cell array of underlying `MException`s (chained causes).
  - `Correction` — suggested fixes (newer releases).
- Methods:
  - `throw(ME)` — raise it; stack originates at the throw site.
  - `throwAsCaller(ME)` — raise it as though it occurred in the **caller** (hides this
    function's frame — cleaner user-facing errors from validation helpers).
  - `rethrow(ME)` — re-raise a **caught** exception preserving its **original** stack.
  - `addCause(ME, causeME)` — returns a new exception with `causeME` appended to `cause`
    (chaining root causes).
  - `getReport(ME)` — formatted multi-line report string (message + stack), as the default
    display would show.

Ref: https://www.mathworks.com/help/matlab/ref/mexception.html

### 6.4 `try` / `catch` / `end`
```matlab
try
    stmts
catch ME              % ME is optional; if present it binds the MException
    handling stmts
end
```
- On any error in the `try` block, control jumps **immediately** to `catch`; remaining `try`
  statements are skipped. If `catch VAR` names a variable, it receives the current
  `MException`. `catch` **without** a variable is legal (error info discarded but catchable).
- **No `finally`** clause exists in MATLAB.
- **One `catch` per `try`** — no multiple typed catch blocks. To discriminate error types,
  inspect `ME.identifier` inside the catch (typically a `switch`/`if` on the id) and
  `rethrow(ME)` those you don't handle.
- `try`/`catch` **nest** arbitrarily (either block may contain full `try/catch` blocks).
- `assert` failures and thrown `error`s inside a `try` are routed to its `catch`.
- Legacy `lasterror` / `lasterr` (function-based last-error retrieval) still exist but are
  **discouraged** in favor of `catch ME`; `lasterror` returns a struct (`message`,
  `identifier`, `stack`). Implement for compatibility but treat as legacy.

Ref: https://www.mathworks.com/help/matlab/ref/try.html

### 6.5 `assert()`
```matlab
assert(cond)                       % error 'Assertion failed.' if cond false
assert(cond, msg)                  % custom message
assert(cond, formatMsg, A1, ...)   % sprintf-formatted message
assert(cond, errID, formatMsg, A1) % with identifier
```
- `cond` uses the truthiness rule. If false, throws an error (routed to any enclosing `catch`).
- Same ≥2-arg rule for escape/format-spec processing as `error`.

Ref: https://www.mathworks.com/help/matlab/ref/assert.html

### 6.6 `warning()`
```matlab
warning(msg)
warning(formatMsg, A1, ...)          % sprintf formatting (needs ≥2 args)
warning(errID, formatMsg, A1, ...)
warning('on'|'off', errID)           % enable/disable a specific warning by id
warning('on'|'off')                  % all warnings
w = warning('query', errID)          % query current state; returns struct
s = warning;  ...  warning(s);        % save & restore all warning states
```
- Prints to stderr and **continues execution** (does not unwind). Same identifier-format
  rules as `error` (component:mnemonic).
- **On/off state per identifier**: a disabled id produces no output. State is process-global
  and stackable via the struct save/restore idiom (common in RAII-style cleanup with
  `onCleanup`).
- Extra modes: `warning('backtrace','on'/'off')`, `warning('verbose','on'/'off')`.
- `lastwarn` retrieves the last warning's message + identifier (and can reset it).

Ref: https://www.mathworks.com/help/matlab/ref/warning.html

---

## 7. Recursion, Function Handles to Local Functions, `feval`

### 7.1 Recursion
- Functions may call themselves. Each call gets its own workspace (§5.1); `persistent`
  variables are shared across the recursion (same function). Deep recursion hits MATLAB's
  recursion limit (`set(0,'RecursionLimit',N)`, default 500) → error, not silent overflow.

### 7.2 Function handles to local / nested functions
- Inside a file, `h = @localFn;` creates a handle to a **local** function; it can be passed
  out of the file and invoked elsewhere even though `localFn` isn't otherwise externally
  visible.
- `h = @nestedFn;` creates a handle to a **nested** function that also carries the shared
  parent workspace (see §5.5) — a true closure over live variables.
- Handles capture the function **identity at creation**; they follow normal scoping/precedence
  when invoked.

### 7.3 `feval`
```matlab
[y1,...,yN] = feval(fun, x1,...,xM)   % fun = name (char) or function handle
```
- Evaluates `fun` with the given args; identical to invoking the handle directly
  (`fun(x1,...)`), so with a handle `feval` is redundant.
- **Follows the same scoping & precedence rules** as a direct call.
- To feval a **local or nested** function you must pass a **handle** (`@localFn`), not the
  name string — name resolution via string can't reach local/nested/private scopes.

Refs: https://www.mathworks.com/help/matlab/ref/feval.html ,
https://www.mathworks.com/help/matlab/ref/function_handle.html

---

## 8. Implementation Checklist (MATBOX)

- [ ] Single truthiness routine (nonempty AND all-nonzero; empty=false; NaN=true; complex→real part) used by `if`/`elseif`/`while`/`&&`/`||`.
- [ ] `for` iterates **columns** of the RHS matrix/cell/struct; range operator builds a row vector; index reset each iteration (body assignment ignored); range evaluated once.
- [ ] `switch`: no fall-through; number `==`, char/string `strcmp`, cell-case "any element matches", object `eq`.
- [ ] `break`/`continue` bind to innermost loop; `return` unwinds whole function; `parfor` forbids `break`/`return`/early exit.
- [ ] Function workspaces isolated; pass-by-value with copy-on-write; handle classes are references.
- [ ] `global` requires per-workspace declaration; `persistent` inits to `[]`, retained across calls, local, cleared with the function; declaration precedes use.
- [ ] Nested functions: shared-workspace by static usage analysis; require `end` on all file functions; no defn inside control blocks; handles are live closures; not reachable by `feval`-by-name.
- [ ] `nargin`/`nargout` inside call = actual/requested counts; query-by-name returns declared counts, negative when `varargin`/`varargout` present; expose `nargout` to enable lazy multi-output behavior; `~` output ignoring.
- [ ] `varargin`/`varargout` last-position cell aggregation; `varargin{:}` CSL expansion; `inputname` returns caller var name or `''`.
- [ ] `arguments` block: top-of-function; size→class(convert)→validators; defaults ⇒ optional & after required; name-value via `options.Field`; `(Repeating)` cell aggregation; `(Output)` no defaults; unsupported in nested fns.
- [ ] Error IDs `component:mnemonic` (letter-start fields, no whitespace, ≥1 colon); `error`/`warning`/`assert` process escapes/format-specs **only with ≥2 args**; all-empty `error` is a no-op.
- [ ] `MException` (throw/throwAsCaller/rethrow/addCause/getReport, cause chaining, stack); `try`/single-`catch` (no `finally`); `catch ME` optional; nest freely; legacy `lasterror`/`lastwarn` for compat.
- [ ] `feval` name vs handle; local/nested/private only reachable via handle; recursion limit.
