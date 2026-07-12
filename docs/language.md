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
| `'`         | 110       | left  | conjugate transpose     |
| `.'`        | 110       | left  | non-conjugate transpose |
| `+` `-`     | 100       | left  | unary plus/minus        |
| `.*`        | 90        | left  | element-wise multiply   |
| `./`        | 90        | left  | element-wise divide     |
| `.\`        | 90        | left  | element-wise left-divide|
| `*`         | 80        | left  | matrix multiply         |
| `/`         | 80        | left  | matrix divide           |
| `\`         | 80        | left  | matrix left-divide      |
| `+` `-`     | 60        | left  | addition/subtraction    |
| `:`         | 20        | left  | colon range             |
| `<` `<=`    | 10        | left  | relational              |
| `>` `>=`    | 10        | left  | relational              |
| `==` `~=`   | 10        | left  | equality                |
| `&`         | 5         | left  | logical AND             |
| `|`         | 3         | left  | logical OR              |
| `&&`        | 4         | left  | short-circuit AND       |
| `||`        | 2         | left  | short-circuit OR        |

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
| `Array` | N-D complex-valued matrix (the universal type) |
| `String` | Character string |
| `Cell` | Heterogeneous array (`Vec<Vec<Value>>`) |
| `Struct` | Field-value map (`HashMap<String, Value>`) |
| `Func` | Anonymous function handle (captured environment) |
| `FuncRef` | Named function handle (`@sin`) |
| `Multi` | Multi-output value container |

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
sprintf, contains, startsWith, endsWith
```

### Type Checking

```
ischar, islogical, isnumeric, isreal, isscalar
isvector, isrow, iscolumn, ismatrix, isempty
isinf, isnan, isfinite, iscell, isstruct
```

### Cell Operations

```
cellfun, num2cell, cell2mat, cat
```

### Utilities

```
disp, error, warning, who, clear, exist
format, run, help, feval, input
struct, isfield, fieldnames, rmfield, orderfields
isequal, isequaln
deal
nargin, nargout
tic, toc, clc, pause
```

---

## 7. Constants

| Name    | Value                        |
|---------|------------------------------|
| `pi`    | 3.141592653589793            |
| `eps`   | 2.220446049250313e-16        |
| `inf`   | ∞                            |
| `nan`   | NaN                          |
| `i`     | 0 + 1i                       |
| `j`     | 0 + 1i                       |
| `true`  | 1                            |
| `false` | 0                            |

Constants are stored in the workspace and can be shadowed.
Call `clear` to reset.

---

## 8. MATBOX Extensions

MATBOX extends standard MATLAB syntax with:

| Feature | Syntax |
|---------|--------|
| Double-quoted strings | `"hello"` |
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
