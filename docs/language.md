# MATBOX Language Specification (.mbox)

**Version**: 0.1.0  
**File extension**: `.mbox`  
**Paradigm**: Multi-paradigm: imperative, procedural, functional, array-oriented  
**Memory**: Automatic (workspace-based)  
**Typing**: Dynamic, everything is an `MxArray` (complex-valued matrix)

MATBOX is a clean-room, from-scratch reimplementation of the MATLAB language.
The `.mbox` format is a superset of `.m` — all `.m` files should parse, and
MATBOX adds its own extensions (like `endfunction` and module imports).

---

## 1. Lexical Structure

### Comments

```
%  single-line comment  (preferred)
#  single-line comment  (MATBOX extension)

%{
  multi-line comment
%}
```

### Identifiers

```
letter   ::= [a-zA-Z_]
ident    ::= letter (letter | digit)*
```

Identifiers are case-sensitive. `foo`, `Foo`, and `FOO` are distinct.

### Keywords

```
if, elseif, else, end, endfunction, for, while, break, continue,
function, return, switch, case, otherwise, try, catch,
parfor, persistent, global, true, false, pi, eps, inf, nan, i, j
```

`true`, `false`, `pi`, `eps`, `inf`, `nan`, `i`, `j` are **reserved constants**
stored in the workspace. They can be shadowed by user assignment.

### Literals

```
num     ::= digit+ ('.' digit+)? ([eE] [+-]? digit+)?
imag    ::= num [ij]               → pure imaginary:  2i, 3.14j
str     ::= "'" {char} "'"         → single-quoted string
str     ::= '"' {char} '"'         → double-quoted string  (MATBOX extension)

numeric ::= 42, 3.14, 1e-10, 2i, 3.14j
string  ::= 'hello', "world"
```

### Operators

| Symbol      | Precedence | Assoc | Name                    |
|-------------|-----------|-------|-------------------------|
| `^` `**`    | 120       | right | matrix power            |
| `.^`        | 120       | right | element-wise power      |
| unary `+` `-` | 111     | right | sign (binds looser than `^`, tighter than `*`) |
| `'`         | 110       | left  | conjugate transpose     |
| `.'`        | 110       | left  | non-conjugate transpose |
| `*`         | 110       | left  | matrix multiply         |
| `/`         | 110       | left  | matrix divide           |
| `\`         | 110       | left  | matrix left-divide      |
| `.*`        | 110       | left  | element-wise multiply   |
| `./`        | 110       | left  | element-wise divide     |
| `.\`        | 110       | left  | element-wise left-divide|
| `+` `-`     | 100       | left  | addition/subtraction    |
| `<` `<=` `>` `>=` `==` `~=` | 80 | left | relational          |
| `&`         | 70        | left  | logical AND             |
| `\|`        | 60        | left  | logical OR              |
| `&&`        | 50        | left  | short-circuit AND       |
| `\|\|`      | 50        | left  | short-circuit OR        |
| `:`         | 10        | left  | colon range             |

Because a leading sign sits at 111, it applies to the whole power rather than
to the base: `-2^2` is `-(2^2)`, which evaluates to `-4`. Parentheses
(`(-2)^2` → `4`) or an explicit sign still force the other grouping.

`~` (logical NOT) binds tighter than every operator above.

### Punctuation

| Token | Name          |
|-------|---------------|
| `;`   | statement end / row separator |
| `,`   | expression separator / column separator |
| `=`   | assignment |
| `()`  | function call / grouping |
| `[]`  | matrix / array literal |
| `{}`  | cell array literal |
| `...` | line continuation |
| `@`   | anonymous function / function handle |
| `.`   | struct field access / decimal point |
| `:`   | colon range / all-elements |

---

## 2. Types

MATBOX has a unified core type: **`MxArray`** — an N-dimensional array of
complex numbers (`Complex64`).

Additional wrapper types:

| Type | Description |
|------|-------------|
| `Array` | N-D matrix (`MxArray`); storage is double, logical, complex or int64 |
| `String` | Character row vector (`'text'`) |
| `Str` | String type (`"text"`) — a grid of text elements, distinct from `String` |
| `Cell` | Heterogeneous array (`Vec<Vec<Value>>`) |
| `Struct` | Field-value map (`HashMap<String, Value>`) |
| `Map` | `containers.Map` — ordered key/value entries with arbitrary keys |
| `Func` | Anonymous function handle (captured environment) |
| `FuncRef` | Named function handle (`@sin`) |
| `Multi` | Multi-output value container |

The char and string types are separate: `'ab'` is a 2-element char row vector
while `"ab"` is a single string element. Bracketing char arrays splices their
characters (`['ab','cd']` → `'abcd'`), bracketing strings builds a string array
(`["ab","cd"]` → two elements), and `+` appends text on strings. `class(x)`
reports which one a value is.

Concatenating arrays preserves the logical class when every operand is logical,
and `class` reports `int64` for every integer width — the engine stores all
integer widths in one integer representation.

---

## 3. Expressions

### Arithmetic

```matlab
a + b      % addition (broadcast)
a - b      % subtraction
a * b      % matrix multiply
a .* b     % element-wise multiply
a / b      % matrix right-divide (a * inv(b))
a ./ b     % element-wise divide
a ^ n      % matrix power
a .^ n     % element-wise power
a \ b      % matrix left-divide (inv(a) * b)
a'         % conjugate transpose
a.'        % non-conjugate transpose
```

### Relational

```matlab
a == b     % equal
a ~= b     % not equal
a < b      % less than
a <= b     % less or equal
a > b      % greater than
a >= b     % greater or equal
```

All relational operators work element-wise and return logical arrays (1.0 / 0.0).

### Logical

```matlab
a & b      % element-wise AND
a | b      % element-wise OR
~a         % logical NOT
a && b     % short-circuit AND (scalar only)
a || b     % short-circuit OR (scalar only)
```

### Colon Range

```matlab
1:10              % [1 2 3 4 5 6 7 8 9 10]
1:2:10            % [1 3 5 7 9]
10:-1:1           % [10 9 8 7 6 5 4 3 2 1]
A(:, :)           % all rows and columns
A(end)            % last element
A(end-1:end)      % second-to-last and last
```

### Function Calls

```matlab
sin(x)
cos(pi)
A(1, 2)           % matrix indexing (also function call if A is a function)
f = @(x) x^2; f(3)   % anonymous function call
f = @sin; f(pi/2)     % named function handle
```

### Anonymous Functions

```matlab
f = @(x) x^2
g = @(x, y) x + y
h = @() 42
nested = @(x) @(y) x + y   % closures with capture
```

### Matrix Literals

```matlab
A = [1 2 3; 4 5 6]    % 2×3 matrix: rows separated by ;
B = [1, 2, 3]         % 1×3 row vector
C = [1; 2; 3]         % 3×1 column vector
D = []                 % empty matrix 0×0
```

### Cell Arrays

```matlab
C = {1, 'hello', @sin}        % 1×3 cell row
C = {1, 2; 3, 4}              % 2×2 cell matrix
val = C{1, 2}                 % brace indexing returns contents
C{1} = 99                     % assignment to cell
```

### Structs

```matlab
s = struct('name', 'test', 'val', 42)
s.name                        % struct field access
s.('name')                    % dynamic field access
s.newfield = 99               % auto-vivification
```

---

## 4. Statements

### Assignment

```matlab
x = 42
A(1, 2) = 99
s.field = 'value'
[a, b] = myfunction(x)       % multi-output
```

Assigning past the current end grows the array instead of failing. The new
positions are zero-filled (or `false` for a logical array, so growing keeps the
array's existing class), and `end` in an assignment refers to the size before
the write, which makes `v(end+1) = x` the idiomatic append:

```matlab
v = [1 2 3];
v(end+1) = 9;      % v == [1 2 3 9]
v(6) = 5;          % v == [1 2 3 0 0 5], gap zero-filled
M = [1 2; 3 4];
M(3,3) = 9;        % 3x3, original block top-left, rest zero
```

Assigning an empty value deletes instead:

```matlab
A = [1 2 3 4];
A(2) = [];         % A == [1 3 4]
A(:) = [];         % A is empty
```

Deletion shrinks a vector and keeps its orientation. Deleting a linear index
out of a matrix has no single resulting shape, so it is reported as an error.

### Conditionals

```matlab
if x > 0
    disp('positive')
elseif x < 0
    disp('negative')
else
    disp('zero')
end
```

### Switch

```matlab
switch value
    case 1
        disp('one')
    case {2, 3, 4}
        disp('small')
    otherwise
        disp('other')
end
```

### Loops

```matlab
for k = 1:10
    disp(k)
end

for val = [10 20 30]
    disp(val)
end

while x > 0
    x = x - 1
end
```

### Parallel Loop (MATBOX Extension)

```matlab
parfor k = 1:10
    disp(k)
end
```

`parfor` is parsed and accepted but currently executes sequentially (no parallelization).

### Loop Control

```matlab
for k = 1:10
    if k > 5, break, end
    if mod(k, 2) == 0, continue, end
    disp(k)
end
```

### Function Definitions

```matlab
function y = square(x)
    y = x^2
end

function [a, b] = swap(x, y)
    a = y
    b = x
end

function hello()
    disp('Hello, MATBOX!')
end

function result = vararg_demo(varargin)
    % varargin packs extra positional args into a cell
    result = varargin;
end

function varargout = vararg_demo2(a, b, varargin)
    % varargout unpacks cell into multiple output variables
    varargout = {a, b, varargin};
end
```

Functions create their own local scope. Variables from the caller workspace
are not visible. Input arguments are passed by value (copy-on-write).
`varargin` and `varargout` can appear at the end of the argument list;
they pack/unpack remaining arguments as cell arrays.

### Try/Catch

```matlab
try
    result = risky_operation()
catch err
    disp(err)
end
```

The catch variable is bound to an exception struct with a `message` field and
an `identifier` field. `error` takes an optional identifier first, so
`error('id:tool:badinput', 'value must be positive')` produces
`err.identifier == 'id:tool:badinput'`; an error raised without one leaves the
identifier empty. `rethrow(err)` re-raises the caught exception with its
identifier intact, which lets an inner block hand a specific failure to an
outer handler.

### Return

```matlab
function result = compute(x)
    if x < 0
        result = NaN
        return
    end
    result = sqrt(x)
end
```

### Persistent Variables

```matlab
function myfunc()
    persistent count
    if isempty(count)
        count = 0
    end
    count = count + 1
    disp(count)
end
```

`persistent` variables retain their value between calls to the same function.
They are initialized to `[]` on the first call.

### Global Variables

```matlab
global MY_FLAG
MY_FLAG = 42

function print_flag()
    global MY_FLAG
    disp(MY_FLAG)
end
```

`global` variables are shared across all functions and the base workspace.
They must be declared with `global` in every scope that accesses them.

---

## 5. Scripts (.mbox files)

A `.mbox` file is a sequence of statements executed in the caller's workspace.
Scripts have no function signature — they are loaded and executed at the call site.

```matlab
% myscript.mbox
x = 42;
disp(x * 2)
```

Run: `matbox run myscript.mbox` or call the name without extension from the REPL.

---

## 6. Built-in Functions

### Elementary Math

```
sin, cos, tan, asin, acos, atan, atan2
sinh, cosh, tanh, asinh, acosh, atanh
exp, log, log10, log2, sqrt
abs, angle, conj, real, imag
floor, ceil, round, fix, sign
mod, rem
```

### Array Operations

```
length, size, numel, ndims
sum, prod, mean, min, max, any, all
cumsum, cumprod, diff
sort, unique, flip, fliplr, flipud
reshape, repmat, kron, diag
find, linspace, logspace
```

`max` and `min` report the position of the extremum as a second output, `sort`
reports the permutation it applied, and `find` reports row and column indices
instead of a linear index:

```matlab
[m, i] = max([3 1 9 2]);   % m == 9, i == 3
[s, p] = sort([3 1 9]);   % p indexes back into the original
[r, c] = find(X);          % r and c, column-shaped
```

A second argument of `[]` is the documented placeholder for "every dimension",
and the axis can be named after it: `max(A, [], 2)`.

### Linear Algebra

```
inv, det, rank, trace, norm
lu, qr, chol, eig, svd, pinv
```

### Matrix Creation

```
zeros, ones, eye, rand, randn, randi, randperm, magic
```

### String Operations

```
strcmp, strcmpi, upper, lower, strtrim, deblank
strsplit, strjoin, num2str, int2str, str2double
sprintf, fprintf, contains, startsWith, endsWith
string, strlength, mat2str
```

`sprintf` builds text and `fprintf` writes it, returning the number of
characters written; a leading file id of `1` or `2` selects stdout or stderr.
Both expand `\n` and `\t` and honour precision (`%.2f`). `mat2str` renders a
value as source text that reads back as the same value, spelling logicals as
`true`/`false` so a round trip preserves the class.

### Type Checking

```
ischar, islogical, isnumeric, isreal, isscalar
isvector, isrow, iscolumn, ismatrix, isempty
isinf, isnan, isfinite, iscell, isstruct
class
```

### Cell Operations

```
cellfun, num2cell, cell2mat, cat
```

### Maps

```
containers.Map, keys, values, isKey, remove, Count
```

A map holds entries keyed by arbitrary values rather than by consecutive
indices. Entries are read and written with call-style indexing on the variable
itself, `m(key)`, which both looks up and inserts. `Count` is available as
`m.Count` or `Count(m)`, and `isKey` reports membership without raising an
error.

### Utilities

```
disp, error, warning, who, clear, exist
format, run, help, feval, input
struct, isfield, fieldnames, rmfield, orderfields
isequal, isequaln
rethrow
deal
nargin, nargout
tic, toc, clc, pause
```

---

## 7. Constants

| Name    | Value                        |
|---------|------------------------------|
| `pi`    | 3.141592653589793            |
| `e`     | 2.718281828459045            |
| `eps`   | 2.220446049250313e-16        |
| `inf` `Inf` | ∞                         |
| `nan` `NaN` `NA` | NaN                   |
| `i`     | 0 + 1i                       |
| `j`     | 0 + 1i                       |
| `true`  | logical 1                    |
| `false` | logical 0                    |

`true` and `false` have the logical class, so `class(true)` is `'logical'`.

The non-finite values are spelled `Inf`, `-Inf` and `NaN` when printed.
Constants are stored in the workspace and can be shadowed.
Call `clear` to reset.

---

## 8. MATBOX Extensions

MATBOX extends standard MATLAB syntax with:

| Feature | Syntax |
|---------|--------|
| Double-quoted strings | `"hello"` |
| `containers.Map` | `containers.Map(keys, values)`, `m(key)`, `m.Count` |
| Exception identifiers | `error('id:tool:msg', 'text')`, `err.identifier` |
| `#` comments | `# this is a comment` |
| `**` power operator | `2 ** 3` |
| `@name` function handles | `@sin`, `@cos` |
| Dynamic field access | `s.('fieldname')`, `s.(expr)` |
| `endfunction` keyword | `endfunction` closes function blocks |
| `parfor` | `parfor i = 1:n` — parsed, sequential fallback |
| `persistent` | `persistent x` — retained across calls |
| `global` | `global x` — shared across scopes |
| `varargin` / `varargout` | `function varargout = f(varargin)` |
| `case {a,b,c}` | Multi-value switch matching |
| `%{...%}` block comments | Multiline block comments |
