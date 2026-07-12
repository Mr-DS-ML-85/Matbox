# MATBOX Language Spec — 02: Data Types / Classes and Semantics

> **Clean-room note.** This document describes *documented* MATLAB behavior in our
> own words, compiled from public MathWorks documentation (`mathworks.com/help`) and
> other public references, for a from-scratch reimplementation. No MathWorks source
> code was consulted or reproduced. All observable-behavior claims are traceable to
> the cited public doc pages. Where the docs are silent, items are marked
> *(observed / undocumented)* and must be treated as low-confidence.

## 0. Overview of the type system

MATLAB is dynamically typed. Every value is an **array** (0-D through N-D); there are
no scalars distinct from 1×1 arrays and no "primitive vs. object" split — even a plain
`3` is a 1×1 `double` matrix. MATLAB documents ~17 *fundamental classes*, all of which
are array-shaped. ([Fundamental MATLAB Classes](https://www.mathworks.com/help/matlab/matlab_prog/fundamental-matlab-classes.html), [Data Types](https://www.mathworks.com/help/matlab/data-types.html))

The fundamental classes:

| Category | Classes |
|---|---|
| Floating point | `double`, `single` |
| Signed integer | `int8`, `int16`, `int32`, `int64` |
| Unsigned integer | `uint8`, `uint16`, `uint32`, `uint64` |
| Logical | `logical` |
| Character / text | `char`, `string` |
| Containers | `cell`, `struct`, `table`, `timetable` |
| Function handle | `function_handle` |
| Categorical / time | `categorical`, `datetime`, `duration`, `calendarDuration` |
| Map (handle class) | `containers.Map` |

**Default rule:** numeric literals and results of numeric expressions are `double`
unless an operand forces another type. `x = 3` makes `x` a `double`. This is the single
most important default an implementer must honor. ([Numeric Types](https://www.mathworks.com/help/matlab/numeric-types.html))

Every value also carries a **size** (row × col × …) and a **class name** returned by
`class(x)`. Arrays are stored column-major.

---

## 1. Numeric types

### 1.1 `double` and `single`

- `double`: IEEE-754 binary64 (64-bit). The universal default numeric type.
- `single`: IEEE-754 binary32 (32-bit). Produced only when you ask (`single(x)`, a
  `single` literal-cast, or an operation with a `single` operand).
- Both support real and **complex** values (see §1.4), `Inf`, `-Inf`, `NaN`, `-0`.
- **Type-promotion rule for float+float:** `single` combined with `double` yields
  `single` (the narrower float wins, because the result cannot be more precise than the
  `single` operand). This is the opposite direction from the usual "widen" intuition and
  must be implemented explicitly. ([Floating-Point Numbers](https://www.mathworks.com/help/matlab/matlab_prog/floating-point-numbers.html))
- `single` combined with any **integer** type is an **error** (only `double`/`logical`/`char`
  scalars may combine with integers — see §1.3).
- Special-value functions: `Inf`, `NaN`, `eps`, `realmax`, `realmin`, `flintmax`
  (largest consecutive integer exactly representable: 2^53 for double, 2^24 for single).

### 1.2 Integer types — ranges

Eight integer classes. Query limits with `intmin(class)` / `intmax(class)`.
([Integers](https://www.mathworks.com/help/matlab/matlab_prog/integers.html))

| Class | Bits | Min (`intmin`) | Max (`intmax`) |
|---|---|---|---|
| `int8`   | 8  | −128 | 127 |
| `int16`  | 16 | −32,768 | 32,767 |
| `int32`  | 32 | −2,147,483,648 | 2,147,483,647 |
| `int64`  | 64 | −9,223,372,036,854,775,808 | 9,223,372,036,854,775,807 |
| `uint8`  | 8  | 0 | 255 |
| `uint16` | 16 | 0 | 65,535 |
| `uint32` | 32 | 0 | 4,294,967,295 |
| `uint64` | 64 | 0 | 18,446,744,073,709,551,615 |

### 1.3 Integer semantics — **SATURATION** (critical)

Implementers get this wrong: **MATLAB integer arithmetic and conversion SATURATE
(clamp), they do NOT wrap around** like C. ([Integers](https://www.mathworks.com/help/matlab/matlab_prog/integers.html))

- **On conversion:** `int8(200)` → `127`; `int8(-200)` → `-128`; `uint8(-5)` → `0`;
  `uint8(300)` → `255`. Any value above `intmax` clamps to `intmax`, any value below
  `intmin` clamps to `intmin`.
- **On arithmetic overflow:** `int8(100) + int8(100)` → `127` (not −56). `uint8(200) +
  uint8(100)` → `255`. Every arithmetic op that exceeds the range clamps to the nearest
  representable endpoint.
- This saturation applies when **executing MATLAB code** (Command Window / scripts /
  functions). It is the fixed, non-configurable behavior of the language. (In *code
  generation* the "saturate on integer overflow" behavior is a *configurable* option, but
  that is a codegen concern, not the language semantics —
  [saturateonintegeroverflow](https://www.mathworks.com/help/coder/ref/saturateonintegeroverflow.html).)

**Rounding on conversion from float → integer:** round to **nearest**, and **ties go
away from zero** (round-half-away, a.k.a. round-half-up in magnitude): `int8(2.5)` → `3`,
`int8(-2.5)` → `-3`, `int8(2.4)` → `2`. Use `fix`/`floor`/`ceil`/`round` beforehand for
other rounding. Note MATLAB's `round` default is also round-half-away-from-zero (unlike
IEEE round-half-even). ([Integers](https://www.mathworks.com/help/matlab/matlab_prog/integers.html))

**Mixing integers with other classes:**

| Left operand | Right operand | Result | Notes |
|---|---|---|---|
| integer type T | same integer type T | T | ok |
| integer type T | `double` scalar/array | **T** | double operand allowed; result saturates & rounds into T |
| integer type T | `logical` / `char` | T | treated like double for the math |
| integer type T | **different** integer type | **ERROR** | e.g. `int8(1)+int16(1)` errors: "Integers can only be combined with integers of the same class, or scalar doubles." |
| integer type T | `single` | **ERROR** | single is not allowed with integers |

So the result of an integer/double operation takes the **integer** class (documented:
arithmetic combining an integer with a floating-point number yields the integer type),
and the mathematical result is computed then rounded (nearest, ties away from 0) and
saturated. Only **scalar** doubles combine with integer *arrays* in the general case;
same-size double arrays also combine, but a differently-sized non-scalar double is a
size error like any elementwise op. ([Integers](https://www.mathworks.com/help/matlab/matlab_prog/integers.html), [Combining Unlike Integer Types](https://www.mathworks.com/help/matlab/matlab_prog/combining-unlike-integer-types.html))

- **64-bit precision note:** operations combining a 64-bit integer (`int64`/`uint64`) with
  a scalar `double` are documented to use 80-bit extended precision internally to avoid
  precision loss, then round/saturate into the 64-bit result. ([Integers](https://www.mathworks.com/help/matlab/matlab_prog/integers.html))
- **Integers cannot be complex.** Applying integer math to complex operands is unsupported.

### 1.4 Integer division rounding

Division with the ordinary `/`, `./`, `\`, `.\` operators on integers **rounds the
quotient to the nearest integer, ties away from zero** — i.e. `A./B` for integers is
equivalent to `idivide(A,B,'round')`. Example: `int32(9)/int32(4)` → `2` (2.25 → 2);
`int32(11)/int32(4)` → `3` (2.75 → 3); `int32(10)/int32(4)` → `3` (2.5 → ties away → 3).

`idivide(A,B)` is different: its **default option is `'fix'`** (round toward zero, i.e.
truncate). Options: `'fix'` (toward zero, default), `'floor'` (toward −∞), `'ceil'`
(toward +∞), `'round'` (nearest, ties away). Operands to `idivide` must be the same
integer class (or one a scalar double). ([idivide](https://www.mathworks.com/help/matlab/ref/idivide.html))

> Gotcha: `a/b` and `idivide(a,b)` give **different** results for integers because the
> operator rounds-to-nearest while `idivide` truncates by default.

### 1.5 Complex numbers

- Any `double`/`single` array may be complex; created via `1i`/`1j`, `complex(re,im)`,
  or any op producing an imaginary part (e.g. `sqrt(-1)`, `(-8)^(1/3)`).
- `complex(a)` forces a complex representation even with zero imaginary part;
  `isreal(complex(3))` is `false` (the array *stores* an imaginary part even if 0).
- `real`, `imag`, `conj`, `abs`, `angle` operate on complex arrays.
- `isreal(x)` returns `true` only if `x` has **no** allocated imaginary part — an
  operation that yields an all-zero imaginary part may still auto-narrow to real
  (MATLAB drops a zero imaginary part in many ops). Implementers must track the
  "is stored complex" flag separately from "has nonzero imaginary values."
  ([isreal](https://www.mathworks.com/help/matlab/ref/isreal.html), [complex](https://www.mathworks.com/help/matlab/ref/complex.html))
- Integer and `logical` types cannot hold complex values.

---

## 2. `logical`

- Values `true` / `false`, stored as 1 byte each; `class` is `logical`.
- **Produced by:** relational operators (`==`, `~=`, `<`, `>`, `<=`, `>=`), logical
  operators (`&`, `|`, `~`, `&&`, `||`, `xor`), the `is*` predicate functions, `true`,
  `false`, `logical(x)` (nonzero → true, zero → false; `NaN`→ error under `logical`).
- **Logical indexing (mask indexing):** a logical array used as a subscript selects
  elements where the mask is `true`. `A(A>0)` returns a column vector of the positive
  elements. The mask should match the array size (or be applied along a dimension).
  Assignment form `A(mask) = 0` sets masked elements. This is distinct from numeric
  (position) indexing: `A([1 0 1])` with a **double** `[1 0 1]` is an *index error*
  (index 0 invalid), whereas `A(logical([1 0 1]))` is a mask. The class of the subscript
  decides the meaning. ([Find Array Elements That Meet a Condition](https://www.mathworks.com/help/matlab/matlab_prog/find-array-elements-that-meet-a-condition.html))
- `if`/`while` conditions test whether **all** elements are nonzero (`if v` ≡ `if all(v(:))`);
  an empty array condition is treated as false.

---

## 3. Text: `char` arrays vs `string` arrays (critical distinction)

### 3.1 Two representations

| Aspect | `char` (single quotes `'...'`) | `string` (double quotes `"..."`, R2016b+) |
|---|---|---|
| Literal | `'hello'` | `"hello"` (double-quote literal syntax added R2017a) |
| Class | `char` | `string` |
| A word of N chars | a **1×N** char array (row vector of chars) | a **1×1** string scalar (one element that *holds* the text) |
| Element = | one character | one whole piece of text |
| `size("hello")` | `[1 5]` | `[1 1]` |
| Char count | `length` or `numel` | `strlength` (NOT `length`, which gives array dims) |
| Underlying codes | UTF-16 code units, `double(ch)` gives code points | text stored internally; `char(str)` to get chars |

([Frequently Asked Questions About String Arrays](https://www.mathworks.com/help/matlab/matlab_prog/frequently-asked-questions-about-string-arrays.html), [Represent Text with Character and String Arrays](https://www.mathworks.com/help/matlab/matlab_prog/represent-text-with-character-and-string-arrays.html))

### 3.2 Concatenation — the big trap

- **Square brackets on char** *append characters*: `['ab' 'cd']` → `'abcd'` (a 1×4 char).
- **Square brackets on string** *build an array*: `["ab" "cd"]` → a **1×2 string array**,
  NOT `"abcd"`.
- **To join string text** use `+` or `strcat`/`append`/`join`: `"ab" + "cd"` → `"abcd"`.
  The `+` operator is overloaded for `string`: if either operand is a string, the other
  is converted to string and text is appended (`"x=" + 5` → `"x=5"`). `+` on `char`
  arrays does **numeric** addition of code points, not concatenation.
- Bracket-concatenating a `char` and a `string`, e.g. `['a' "b"]`, promotes the whole
  result to **string** (`"ab"` as a 1×1). Mixing text classes in `[]` yields a string
  array. ([String Comparisons](https://www.mathworks.com/help/matlab/matlab_prog/string-comparisons.html))

### 3.3 Comparison

- `char`: use `strcmp(a,b)` (whole-string equality → scalar logical). Using `==` on two
  char arrays does **element-wise** code-point comparison and requires equal sizes (or
  scalar expansion), returning a logical array — a classic bug source.
- `string`: `==` / `~=` are **overloaded** to compare text element-by-element across the
  string array, returning a logical array of the array's size (each element = whole-text
  equality). `strcmp` also works on strings. `<`, `>` are **not** defined for string.
- Case-insensitive: `strcmpi`. Partial: `strncmp`, `strncmpi`, `startsWith`, `endsWith`,
  `contains`.

### 3.4 Empty and missing

- `''` is a **0×0 char** (empty, `isempty` → true).
- `""` is a **1×1 string** with zero characters — **not empty as an array**
  (`isempty("")` → **false**); test text length via `strlength("")==0`.
- `string` supports a **`<missing>`** value (`string(NaN)` or `missing`), which propagates
  like `NaN` (comparisons with it are false / return `<missing>`). `char` has no missing
  state. ([Frequently Asked Questions About String Arrays](https://www.mathworks.com/help/matlab/matlab_prog/frequently-asked-questions-about-string-arrays.html))

### 3.5 Which class do functions produce?

- Legacy/most base functions default to **char** (e.g. `sprintf`, `num2str`, `fileread`,
  `input(...,'s')`, `datestr`). Newer/`string`-aware functions and any op touching a
  `string` produce **string**. `string(x)` converts; `char(str)` converts back
  (a string array of differing lengths → padded char matrix, or a cell of char via
  `cellstr`). A **cell array of character vectors** (`{'a','bb'}`) is the pre-R2016b idiom
  for "vector of strings" and still appears widely; `cellstr`/`string` convert between them.

---

## 4. `cell` arrays

Heterogeneous container; each element (cell) holds an arbitrary value of any class/size.
([cell](https://www.mathworks.com/help/matlab/ref/cell.html), [Access Data in a Cell Array](https://www.mathworks.com/help/matlab/matlab_prog/access-data-in-a-cell-array.html))

### 4.1 Construction

- `c = {1, 'two', [3 4 5]}` → 1×3 cell.
- `c = cell(m,n)` → m×n cell of `[]` (empty double) placeholders.
- Nesting: `{ {1,2}, 3 }` — a cell may contain cells.

### 4.2 Container vs. content indexing — `()` vs `{}` (critical)

| Syntax | Returns | Meaning |
|---|---|---|
| `c(i)` / `c(i,j)` | a **cell** (sub-cell-array) | *container* indexing — keeps the cell wrapper |
| `c{i}` / `c{i,j}` | the **contents** of that cell (its actual value) | *content* indexing — unwraps |
| `c{i:j}` (multiple) | a **comma-separated list** of contents | expands into multiple values |

- `c(2)` → `{'two'}` (a 1×1 cell). `c{2}` → `'two'` (the char array itself).
- **Comma-separated list (CSL):** `c{1:3}` produces *N separate values*, not one array.
  You can't assign a multi-cell `{}` result to a single variable
  (`x = c{1:3}` only captures the first / errors depending on context); instead:
  `[a,b,d] = c{1:3}` distributes them, `[c{:}]` concatenates them into an array,
  `f(c{:})` passes them as separate arguments (the standard "unpack args" idiom).
  ([curly braces](https://www.mathworks.com/help/matlab/ref/curlybraces.html))
- Assignment: `c{i} = v` stores `v` into cell i; `c(i) = {v}` does the same via container
  form; `c(i) = v` (non-cell RHS) is an error.
- Multi-level: `c{1}(2)` indexes into the contents; `c(1){2}` is invalid (you can't `{}`
  a `()` result of a cell in one shot because `()` returns a cell but the chain rules
  differ) — implement chained indexing so `{}` unwraps before further `()`/`{}`.

---

## 5. `struct` and struct arrays

Container whose elements are grouped into **named fields**; every element of a struct
array has the **same set of field names in the same order**.
([struct](https://www.mathworks.com/help/matlab/ref/struct.html), [Structure Arrays](https://www.mathworks.com/help/matlab/matlab_prog/create-a-structure-array.html))

### 5.1 Construction & fields

- `s.name = 'Ann'; s.age = 30;` — builds a 1×1 struct with fields `name`, `age`.
- `s = struct('a',1,'b',2)`.
- **Field order** is the insertion order; preserved and observable via `fieldnames(s)`.
  `orderfields` reorders.
- Add/remove: assign a new field name to add; `rmfield(s,'x')` to remove.
- `isfield(s,'x')`, `fieldnames(s)`, `numel(fieldnames(s))`.

### 5.2 Dynamic field names

`s.(expr)` where `expr` is a char/string naming a field: `s.('age')` ≡ `s.age`;
`fn='age'; s.(fn)`. Enables computed field access and iteration over `fieldnames`.
([Generate Field Names from Variables](https://www.mathworks.com/help/matlab/matlab_prog/generate-field-names-from-variables.html))

### 5.3 Struct arrays (size > 1) — the sharp edge

- `s(2).name = 'Bob'` grows `s` to a 1×2 struct array; the previously-unset field
  `s(2).age` becomes `[]` (empty). All elements share the field set.
- **`s.field` on a multi-element struct array returns a comma-separated list** (like `{}`
  on a cell): `s.name` with a 1×3 `s` yields 3 separate values. Collect them:
  - `{s.name}` → 1×3 cell of the values,
  - `[s.age]` → concatenation into a numeric row (works when values are compatible scalars),
  - `[a,b,c] = s.name` → distribute.
- **`struct('x', {1,2,3})`** creates a **1×3 struct array** (one element per cell),
  because a *cell-valued* argument is spread across elements. `struct('x', [1 2 3])`
  instead makes a **1×1** struct whose `x` field is the vector `[1 2 3]`. This cell-spread
  rule is a common source of surprise. To store an actual cell in a field via `struct`,
  wrap it twice: `struct('x', {{1,2,3}})`.

### 5.4 `arrayfun` / `structfun`

- `arrayfun(@(e) e.age, s)` applies a function to **each element** of the struct array,
  returning an array of results (add `'UniformOutput',false` to collect non-scalar/mixed
  results into a cell). ([arrayfun](https://www.mathworks.com/help/matlab/ref/arrayfun.html))
- `structfun(@f, s)` applies `f` to each **field** of a scalar struct.

---

## 6. Function handles (`function_handle`)

A first-class value referencing a function or holding an inline expression.
([Function Handles](https://www.mathworks.com/help/matlab/function-handles.html), [Anonymous Functions](https://www.mathworks.com/help/matlab/matlab_prog/anonymous-functions.html))

### 6.1 Two kinds

- **Named handle:** `h = @sin;` — a reference resolved to the function `sin`; called
  `h(x)`. Resolution is bound at *creation* time by the usual function-precedence rules.
- **Anonymous:** `f = @(x) x.^2 + b;` — a parameter list `(x)` plus one expression.
  Multiple params `@(x,y) ...`; zero params `@() rand`.

### 6.2 Variable capture — **by value at creation time** (critical)

- An anonymous function **captures the values of the free variables** (those used in the
  body but not in the parameter list) **at the moment the handle is created**. It is a
  *snapshot*, not a live reference.
- `b = 10; f = @(x) x + b; b = 999; f(1)` → **11**, not 1000. Reassigning or `clear`-ing
  `b` afterward does not change `f`. The captured `b=10` travels with the handle.
- Only variables **actually referenced** in the body are captured; unused workspace
  variables are not stored.
- Capture is by value for value types (numeric/cell/struct/char/string). (Handle-class
  objects are captured by their handle, so their mutable state is shared — an
  object-semantics subtlety.)
- Nested functions differ: they share the parent's workspace *live* (true lexical
  closure), unlike anonymous functions' value-snapshot. ([Anonymous Functions](https://www.mathworks.com/help/matlab/matlab_prog/anonymous-functions.html))

### 6.3 Introspection & conversion

- `func2str(h)` → char representation (`'@(x)x.^2+b'` for anonymous; `'sin'` or `'@sin'`
  for named). `str2func('@(x) x+1')` builds an anonymous handle; `str2func('sin')` builds
  a named handle. **`str2func` handles are NOT closures** — they capture no workspace, so
  a name in the body must resolve at call time. ([func2str](https://www.mathworks.com/help/matlab/ref/func2str.html), [str2func](https://www.mathworks.com/help/matlab/ref/str2func.html))
- `functions(h)` returns a struct with the handle's type, function name, file, and the
  **captured workspace** values (documented as for debugging only). ([functions](https://www.mathworks.com/help/matlab/ref/functions.html))
- `is_function_handle(h)` / `isa(h,'function_handle')` test the class.

---

## 7. Higher-level types (existence + core semantics)

These are documented as fundamental/near-fundamental but implemented as MATLAB classes;
for MATBOX, model the *semantics*, depth secondary.

| Type | Core semantics | Docs |
|---|---|---|
| `categorical` | Array of values drawn from a finite set of named **categories**. Categories can be **ordinal** (ordered, allowing `<`/`>`/`sort` by category rank) or unordered (only `==`/`~=`). Stores compact integer codes + a category list. `<undefined>` is the missing element. Created by `categorical(...)`, `categories`, `iscategory`, `mergecats`, `renamecats`. Efficient for repeated labels. | [categorical](https://www.mathworks.com/help/matlab/ref/categorical.html) |
| `table` | Column-oriented container: N **variables** (columns), each a differently-typed equal-height array; M rows. Access by variable name `T.Var`, by `{}` (extract data) vs `()` (sub-table), row names optional. Supports `join`, `groupsummary`, `sortrows`, `varfun`, `rowfun`. `m>=0`, `n>=0`. | [table](https://www.mathworks.com/help/matlab/ref/table.html) |
| `timetable` | A `table` whose rows are indexed by a `datetime`/`duration` **row-time** vector; adds time-based ops: `retime`, `synchronize`, time-range subscripting, `withtol`. | [timetable](https://www.mathworks.com/help/matlab/ref/timetable.html) |
| `datetime` | A point in time (year/month/day/hour/…), with optional `TimeZone` and display `Format`. Subtracting two `datetime`s gives a `duration`; adding a `duration`/`calendarDuration` shifts. Missing = `NaT`. | [datetime](https://www.mathworks.com/help/matlab/ref/datetime.html) |
| `duration` | An elapsed length of time in fixed units (days/hours/minutes/seconds), stored numerically. Arithmetic with `datetime` and scalars. Constructors `seconds`, `minutes`, `hours`, `days`, `years`, `duration`. Missing = `NaN`. | [duration](https://www.mathworks.com/help/matlab/ref/duration.html) |
| `calendarDuration` | Calendar-aware span (months/years that vary in length); `caldays`, `calmonths`, `calyears`. Distinct from `duration` (fixed length). | [calendarDuration](https://www.mathworks.com/help/matlab/ref/calendarduration.html) |
| `containers.Map` | Hash map from keys (`char`/`string`/numeric scalar) to values of any type. `m(key)=v`, `m(key)`, `keys(m)`, `values(m)`, `isKey`, `remove`, `m.Count`. It is a **handle class**: assigning `m2 = m` shares one underlying map (mutations visible through both) — reference semantics, unlike every value type above. (MathWorks now recommends `dictionary` (R2022b+) as the value-semantics successor.) | [containers.Map](https://www.mathworks.com/help/matlab/ref/containers.map.html) |

---

## 8. Type query & conversion functions

### 8.1 Query

| Function | Returns |
|---|---|
| `class(x)` | char row vector naming the class, e.g. `'double'`, `'int8'`, `'char'`, `'string'`, `'cell'`, `'struct'`, `'function_handle'`. |
| `isa(x,'name')` | logical; true if `x` is that class **or a subclass**. Also accepts category abstractions: `'numeric'` (any int/float), `'float'` (double/single), `'integer'` (any int type). ([isa](https://www.mathworks.com/help/matlab/ref/isa.html)) |
| `metaclass(x)` / `?ClassName` | meta.class object (OO reflection). |

### 8.2 The `is*` family (each returns logical, scalar unless noted)

([Use is* Functions to Detect State](https://www.mathworks.com/help/matlab/matlab_prog/use-is-functions-to-detect-state.html))

| Predicate | True when |
|---|---|
| `isnumeric` | class is a numeric type (int or float). **`logical` and `char` are NOT numeric.** |
| `isfloat` | `double` or `single`. |
| `isinteger` | one of the 8 integer classes (NOT a `double` that holds an integer value). |
| `isreal` | array has no stored imaginary part (see §1.5 caveat). |
| `ischar` | `char` array. |
| `isstring` | `string` array (R2016b+). |
| `iscellstr` | cell array in which **every** element is a char row vector. |
| `iscell` | `cell` array. |
| `isstruct` | `struct` (scalar or array). |
| `islogical` | `logical` array. |
| `isa(x,'function_handle')` | function handle (there is no `isfunc`; `is_function_handle` exists). |
| `iscategorical`, `istable`, `istimetable`, `isdatetime`, `isduration` | respective classes. |
| `isempty` | any dimension is 0 (`numel==0`). Note `""` is **not** empty (§3.4). |
| `isscalar` | size is 1×1 (`numel==1`). |
| `isvector` | 1×N or N×1. |
| `ismatrix` | 2-D (≤ 2 non-singleton... actually `ndims<=2`). |
| `isrow` / `iscolumn` | 1×N / N×1. |
| `isnan`, `isinf`, `isfinite` | element-wise logical array over the input. |

### 8.3 Conversion

- `double(x)`, `single(x)`, `int8(x)`…`uint64(x)`, `logical(x)`, `char(x)`, `string(x)`,
  `cell(...)`, `cellstr(x)`, `categorical(x)`, `datetime(x)`, etc. — each converts *into*
  the named class, applying that class's rules (integer conversions round-nearest-ties-
  away and saturate; `char` of a number → the code-point character; `double` of a char →
  its code point).
- **`cast(x, 'type')`** converts `x` to the class named by the string, honoring saturation
  for integer targets — a dynamic-typed alternative to the fixed converters.
  `cast(x,'like',y)` casts `x` to the class (and complexity) of `y`. ([cast](https://www.mathworks.com/help/matlab/ref/cast.html))
- `typecast(x,'type')` is different: it **reinterprets the underlying bits** without
  changing them (e.g. `typecast(single(1),'uint32')`), requiring matching total byte size.
  Do not confuse `cast` (value-preserving) with `typecast` (bit-preserving).

---

## 9. Default numeric display / formatting

Display format affects **presentation only**, never stored values or precision.
Set per-session with `format <style>`. Default is **`short`**.
([format](https://www.mathworks.com/help/matlab/ref/format.html), [Display Format for Numeric Values](https://www.mathworks.com/help/matlab/matlab_prog/display-format-for-numeric-values.html))

| Style | double | single | Description |
|---|---|---|---|
| `short` *(default)* | 4 digits after decimal (5 significant scaled) | 4 after decimal | fixed-decimal, compact |
| `long` | 15 digits after decimal | 7 after decimal | full precision fixed-decimal |
| `shortE` / `longE` | 4 / 15 mantissa digits | 4 / 7 | scientific (`e` notation) |
| `shortG` / `longG` | 5 / 15 total sig-figs | 5 / 7 | fixed or scientific, whichever is more compact |
| `shortEng` / `longEng` | 4 / 15 sig-figs | — | engineering (exponent a multiple of 3) |
| `bank` | 2 decimals | 2 | currency (dollars & cents) |
| `hex` | — | — | hex of the IEEE bit pattern |
| `rat` | — | — | approximate ratio of small integers |
| `+` | — | — | prints only `+`, `-`, or space (sign map) |

Display rules an implementer must reproduce for parity:

- **Echoing:** an expression not terminated by `;` prints `varname = value` (or `ans =
  value` for unnamed results). A trailing `;` suppresses the echo.
- **Integers always print exactly** (no decimals) regardless of `format`.
- **Column scale factor:** when a matrix's values are large/small, `short`/`long` print a
  common multiplier header like `1.0e+03 *` above the matrix and show the scaled mantissas
  — arrays are shown with a shared exponent, not per-element scientific. This is a
  visible, testable behavior.
- Values needing more room than the fixed format allows switch that column to exponential.
- `NaN`, `Inf`, `-Inf` print as literal `NaN`/`Inf`/`-Inf`. `-0` prints as `0`.
- `logical` prints as `1`/`0`; `char` prints as the text; `string` prints with the text in
  double quotes when it is an array element context; empty arrays print as
  `varname = []` (or e.g. `0×0 empty ... array`).
- `disp(x)` prints the value **without** the `name =` header; `display(x)` includes it.

---

## Implementation checklist (highest-risk items)

1. **Integer overflow SATURATES, never wraps** — clamp every integer op & conversion to
   `[intmin,intmax]`. (§1.3)
2. **Float→int rounding is round-half-away-from-zero**, and MATLAB `round` is too (not
   IEEE round-half-even). (§1.3)
3. **`a/b` on integers rounds to nearest (ties away); `idivide` truncates by default.** (§1.4)
4. **`single`+`double`→`single`**; **`single`+integer = error**; **int T + different int
   T = error**; **int T + scalar double → T**. (§1.1, §1.3)
5. **`char` vs `string`:** `'abc'` is 1×3, `"abc"` is 1×1; `[]` concatenates chars but
   builds string arrays; `+` appends for strings, adds code points for chars; `""` is not
   empty, `''` is. (§3)
6. **`{}` unwraps and yields comma-separated lists; `()` keeps the cell.** Same CSL
   behavior for `s.field` over a struct array. (§4.2, §5.3)
7. **`struct('f',{...})` spreads the cell across struct-array elements.** (§5.3)
8. **Anonymous handles capture free variables by value at creation; `str2func` handles do
   not capture.** (§6.2, §6.3)
9. **`containers.Map` is a handle (reference) type**, unlike all value types. (§7)
10. **`isnumeric` excludes `logical` and `char`; `isinteger` excludes integer-valued
    doubles; `isempty("")` is false.** (§8.2)
11. **`class(x)` returns a char row vector; format affects display only.** (§8.1, §9)

---

### Primary sources

- Fundamental MATLAB Classes — https://www.mathworks.com/help/matlab/matlab_prog/fundamental-matlab-classes.html
- Data Types — https://www.mathworks.com/help/matlab/data-types.html
- Integers — https://www.mathworks.com/help/matlab/matlab_prog/integers.html
- Combining Unlike Integer Types — https://www.mathworks.com/help/matlab/matlab_prog/combining-unlike-integer-types.html
- Floating-Point Numbers — https://www.mathworks.com/help/matlab/matlab_prog/floating-point-numbers.html
- idivide — https://www.mathworks.com/help/matlab/ref/idivide.html
- complex / isreal — https://www.mathworks.com/help/matlab/ref/complex.html , https://www.mathworks.com/help/matlab/ref/isreal.html
- FAQ About String Arrays — https://www.mathworks.com/help/matlab/matlab_prog/frequently-asked-questions-about-string-arrays.html
- Represent Text with Character and String Arrays — https://www.mathworks.com/help/matlab/matlab_prog/represent-text-with-character-and-string-arrays.html
- Compare Text — https://www.mathworks.com/help/matlab/matlab_prog/string-comparisons.html
- curly braces / Access Data in a Cell Array — https://www.mathworks.com/help/matlab/ref/curlybraces.html , https://www.mathworks.com/help/matlab/matlab_prog/access-data-in-a-cell-array.html
- struct / Structure Arrays — https://www.mathworks.com/help/matlab/ref/struct.html , https://www.mathworks.com/help/matlab/matlab_prog/create-a-structure-array.html
- Anonymous Functions / Function Handles — https://www.mathworks.com/help/matlab/matlab_prog/anonymous-functions.html , https://www.mathworks.com/help/matlab/function-handles.html
- func2str / str2func / functions — https://www.mathworks.com/help/matlab/ref/func2str.html , https://www.mathworks.com/help/matlab/ref/str2func.html , https://www.mathworks.com/help/matlab/ref/functions.html
- categorical / table / timetable / datetime / duration / containers.Map — see §7 links
- isa / Use is* Functions / cast — https://www.mathworks.com/help/matlab/ref/isa.html , https://www.mathworks.com/help/matlab/matlab_prog/use-is-functions-to-detect-state.html , https://www.mathworks.com/help/matlab/ref/cast.html
- format / Display Format for Numeric Values — https://www.mathworks.com/help/matlab/ref/format.html , https://www.mathworks.com/help/matlab/matlab_prog/display-format-for-numeric-values.html
