# MATBOX

**MAT**rix **BOX** — a clean-room, from-scratch MATLAB replica written in Rust.

```
  __  __       _ ____   _  __
 |  \/  | __ _| | __ ) | |/ /
 | |\/| |/ _` | |  _ \ | ' / 
 | |  | | (_| | | |_) || . \ 
 |_|  |_|\__,_|_|____(_)_|\_\
```

[![License](https://img.shields.io/badge/license-AGPLv3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blueviolet.svg)](https://crates.io/crates/matbox)
[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/Mr-DS-ML-85/Matbox/actions)
[![Tests](https://img.shields.io/badge/tests-437%20passing-brightgreen.svg)](https://github.com/Mr-DS-ML-85/Matbox)
[![Language](https://img.shields.io/badge/language-.mbox-success.svg)](docs/language.md)
[![Stars](https://img.shields.io/github/stars/Mr-DS-ML-85/Matbox?style=social)](https://github.com/Mr-DS-ML-85/Matbox)

---

## Overview

MATBOX is a fully independent, ground-up reimplementation of the MATLAB
language and runtime. The **compute engine is 100% Rust** — no Python, no
C++, no C. Every number is a complex-valued N-dimensional array, every
operation is vectorized, and every function matches MATLAB semantics.

### Architecture

```
┌──────────────────────────────────────────────┐
│            matbox-cli (REPL / runner)         │
├──────────────────────────────────────────────┤
│              matbox-lsp (LSP server)          │
├──────────────────────────────────────────────┤
│           matbox-core (Engine + Parser)       │
├──────────────────────────────────────────────┤
│           matbox-math (Linear Algebra)        │
├──────────────────────────────────────────────┤
│  matbox-graphics  │  matbox-sim  │  matbox-ffi│
└──────────────────────────────────────────────┘
```

| Crate | Description |
|-------|-------------|
| `matbox-core` | Lexer, parser, AST, interpreter, builtins (325+ tests) |
| `matbox-math` | Column-major `MxArray`, LAPACK-free linalg (QR, eig, SVD, LU, Cholesky) |
| `matbox-cli` | REPL and script runner |
| `matbox-lsp` | Language Server Protocol implementation |
| `matbox-graphics` | WebGL/Canvas plotting backend |
| `matbox-sim` | Simulink-like block diagram solver |
| `matbox-ffi` | C FFI for embedding (`matbox.h`) |

---

## Quick Start

### Installation

```bash
git clone https://github.com/Mr-DS-ML-85/Matbox
cd Matbox
cargo build --release
```

### REPL

```bash
cargo run --release
>> x = [1 2 3; 4 5 6]
>> sin(x)
>> disp('Hello, MATBOX!')
>> exit
```

### Run a script

```bash
cargo run --release -- run examples/demo.mbox
```

### Check syntax

```bash
cargo run --release -- check myscript.mbox
```

---

## Language

MATBOX uses the `.mbox` file extension. The language is a superset of MATLAB:

### Hello World

```matlab
% hello.mbox
disp('Hello, MATBOX!')
```

### Matrix Operations

```matlab
A = [1 2 3; 4 5 6]
B = [7 8 9; 10 11 12]
C = A * B             % matrix multiply
D = A .* B            % element-wise multiply
E = A'                % conjugate transpose
```

### Functions

```matlab
function result = fib(n)
    if n <= 1
        result = n
    else
        result = fib(n-1) + fib(n-2)
    end
end

disp(fib(10))
```

### Anonymous Functions & Closures

```matlab
f = @(x) x^2
f(5)              % 25

adder = @(x) @(y) x + y
add5 = adder(5)
add5(3)           % 8 (closure captures x=5)
```

### Structs

```matlab
s = struct('name', 'MATBOX', 'version', 0.1)
s.description = 'A MATLAB replica in Rust'
disp(s.name)
s.('version')     % dynamic field access
```

### Cell Arrays

```matlab
C = {1, 'hello', @sin}
C{2}              % 'hello'
C{3}(pi/2)        % 1.0
cellfun(@(x) x^2, {1, 2, 3})
```

### Strings & Maps

```matlab
% "text" is a string scalar; 'text' is a char row vector
"ab" + "cd"              % 'abcd'
strlength("hello")       % 5
["ab" "cd"]              % string array, two elements
class("hi")              % 'string'

m = containers.Map();
m('a') = 1;
m('b') = 2;
m.Count                  % 2
keys(m)                  % {'a','b'}
isKey(m, 'a')            % 1
```

### Errors

```matlab
try
    error('id:tool:badinput', 'value must be positive');
catch err
    disp(err.message);      % value must be positive
    disp(err.identifier);   % id:tool:badinput
end
```

---

## Language Features

| Feature | Syntax | Status |
|---------|--------|--------|
| Matrix arithmetic | `+ - * / \\ ^ .* ./ .^ .\\` | ✅ |
| Conjugate transpose | `A'` | ✅ |
| Non-conjugate transpose | `A.'` | ✅ |
| Broadcasting | scalar + matrix, row + col | ✅ |
| Colon ranges | `1:10`, `1:2:10`, `end-1:end` | ✅ |
| Relational | `== ~= < <= > >=` | ✅ |
| Logical | `& \| ~ && \|\|` | ✅ |
| Control flow | `if/elseif/else/end` | ✅ |
| Loops | `for/end`, `while/end`, `break`, `continue` | ✅ |
| Switch | `switch/case/otherwise/end` + cell patterns `case {1,2,3}` | ✅ |
| Try/catch | `try/catch/end` | ✅ |
| Functions | `function [a,b] = name(x,y)/end` | ✅ |
| `endfunction` | MATBOX extension for function terminators | ✅ |
| `varargin` / `varargout` | Variable-length argument lists | ✅ |
| `persistent` | Persistent variables inside functions | ✅ |
| `global` | Global variable sharing | ✅ |
| `parfor` | Parallel for loop (sequential fallback) | ✅ |
| Anonymous functions | `@(x) x^2` | ✅ |
| Closures | `@(x) @(y) x+y` (free variable capture) | ✅ |
| Function handles | `@sin`, `@cos`, `f = @(x) x^2; f(3)` | ✅ |
| Structs | `s.field`, `s.('dynamic')`, `struct()` | ✅ |
| Cell arrays | `{1, 'hello', @sin}`, brace indexing `C{1}` | ✅ |
| Multi-output | `[a, b] = func(x)` | ✅ |
| Multi-output builtins | `[m, i] = max(x)`, `[s, p] = sort(x)`, `[r, c] = find(x)` | ✅ |
| Auto-growing assignment | `v(end+1) = 9`, `v(10) = 1`, `M(3,3) = 9` | ✅ |
| Element deletion | `A(2) = []`, `A(:) = []` | ✅ |
| Dimension placeholder | `max(A, [], 2)` — axis named after `[]` | ✅ |
| Double-quoted strings | `"hello"` (MATBOX extension) | ✅ |
| String operations | `+` appends, `strlength`, `string`, `mat2str` | ✅ |
| `containers.Map` | `containers.Map`, `m(k)`, `keys`, `isKey`, `Count` | ✅ |
| Exception identifiers | `error('id:x', 'msg')`, `err.message`, `err.identifier`, `rethrow` | ✅ |
| `class` | Type name of any value, incl. `'logical'`, `'string'`, `'map'` | ✅ |
| `fprintf` | Formatted output to stdout/stderr | ✅ |
| `#` comments | MATBOX extension | ✅ |
| `**` power operator | `2 ** 3` (MATBOX extension) | ✅ |
| `%{...%}` block comments | Multi-line comment blocks | ✅ |
| Line continuation | `A = [1 2 3; ...` | ✅ |
| LSP server | Completions, diagnostics, hover | ✅ |

## Built-in Functions (220+)

| Category | Functions |
|----------|-----------|
| **Trigonometry** | `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh` |
| **Exponentials** | `exp`, `log`, `log10`, `log2`, `sqrt`, `abs`, `angle`, `conj`, `real`, `imag` |
| **Rounding** | `floor`, `ceil`, `round`, `fix`, `sign`, `mod`, `rem` |
| **Special Math** | `erf`, `erfc`, `erfinv`, `gamma`, `gammaln` |
| **Array** | `length`, `size`, `numel`, `ndims`, `sum`, `prod`, `mean`, `median`, `mode`, `std`, `var`, `min`, `max`, `any`, `all`, `cumsum`, `cumprod`, `diff`, `sort`, `unique`, `flip`, `fliplr`, `flipud`, `reshape`, `repmat`, `kron`, `diag`, `find`, `linspace`, `logspace`, `colon`, `meshgrid` |
| **Linear Algebra** | `inv`, `det`, `rank`, `trace`, `norm`, `lu`, `qr`, `chol`, `eig`, `svd`, `pinv` |
| **Matrix Creation** | `zeros`, `ones`, `eye`, `rand`, `randn`, `randi`, `randperm`, `magic` |
| **Polynomials** | `polyval`, `poly`, `roots`, `polyfit` |
| **Set Operations** | `intersect`, `setdiff`, `setxor`, `union`, `ismember`, `issorted` |
| **Correlation** | `corrcoef`, `cov` |
| **Fourier / Signal** | `fft`, `ifft`, `fftshift`, `ifftshift`, `conv`, `filter` |
| **Numerical** | `fzero`, `trapz`, `integral` |
| **Strings** | `strcmp`, `strcmpi`, `upper`, `lower`, `strtrim`, `deblank`, `strsplit`, `strjoin`, `sprintf`, `fprintf`, `contains`, `startsWith`, `endsWith`, `num2str`, `int2str`, `str2double`, `strfind`, `strrep`, `regexp`, `regexpi`, `regexprep`, `string`, `strlength`, `mat2str` |
| **Cell** | `cellfun`, `num2cell`, `cell2mat`, `mat2cell`, `cell2struct`, `struct2cell`, `cellstr` |
| **Type** | `ischar`, `islogical`, `isnumeric`, `isreal`, `isscalar`, `isvector`, `isrow`, `iscolumn`, `ismatrix`, `isempty`, `iscell`, `isstruct`, `isinf`, `isnan`, `isfinite`, `isa`, `class`, `double`, `single`, `char` |
| **Integer Types** | `int8`, `uint8`, `int16`, `uint16`, `int32`, `uint32`, `int64`, `uint64`, `idivide` |
| **Maps** | `containers.Map`, `keys`, `values`, `isKey`, `remove`, `Count` |
| **Execution** | `eval`, `evalc` |
| **File System** | `pwd`, `ls`, `dir` |
| **Graphics (stubs)** | `figure`, `plot`, `hold`, `subplot`, `xlabel`, `ylabel`, `title`, `grid`, `legend`, `axis`, `bar`, `histogram`, `scatter` |
| **Utilities** | `disp`, `error`, `warning`, `rethrow`, `who`, `clear`, `exist`, `format`, `run`, `help`, `struct`, `isfield`, `fieldnames`, `rmfield`, `orderfields`, `deal`, `feval`, `isequal`, `isequaln`, `cat`, `tic`, `toc`, `input`, `clc`, `pause` |
| **Introspection** | `nargin`, `nargout`, `varargin`, `varargout` |

---

## Language Server

MATBOX includes a built-in LSP server for IDE integration.

```bash
cargo run --release -- lsp
```

Configure your editor to use `matbox lsp` as the language server for `.mbox` files.

### VSCode

Install the **"LSP"** extension (`d0x.assistant-lsp`) or **"vscode-lsp"** from the marketplace, then create `.vscode/settings.json`:

```json
{
  "LSP.languageServers": [
    {
      "name": "matbox",
      "command": ["/path/to/matbox", "lsp"],
      "filetypes": [".mbox"]
    }
  ],
  "files.associations": {
    "*.mbox": "matlab"
  }
}
```

Replace `/path/to/matbox` with the absolute path to the compiled binary (`target/release/matbox` after running `cargo build --release`).

### Neovim (vim-lsp)

```vim
if executable('matbox')
    augroup LspMatbox
        autocmd!
        autocmd User lsp_setup call lsp#register_server({
            \ 'name': 'matbox',
            \ 'cmd': {server_info->['matbox', 'lsp']},
            \ 'whitelist': ['mbox'],
            \ })
    augroup END
endif
```

---

## Project Status

| Feature | Status |
|---------|--------|
| Scalar/Matrix arithmetic | ✅ |
| Linear algebra (eig, QR, SVD, LU, Cholesky) | ✅ |
| Control flow (if/else/for/while/switch/try) | ✅ |
| Function definitions (incl. `varargin`/`varargout`) | ✅ |
| Anonymous functions, closures, function handles | ✅ |
| `persistent` / `global` / `parfor` | ✅ |
| Structs & dynamic field access | ✅ |
| Cell arrays & conversion functions | ✅ |
| String operations (incl. regex) | ✅ |
| `containers.Map` keyed maps | ✅ |
| Exception identifiers (`error`/`catch`/`rethrow`) | ✅ |
| Auto-growing and deleting indexed assignment | ✅ |
| Set operations | ✅ |
| Statistics (std, var, median, mode, corrcoef, cov) | ✅ |
| Signal processing (FFT, conv, filter) | ✅ |
| Polynomials (polyval, roots, polyfit) | ✅ |
| Numerical methods (fzero, trapz, integral) | ✅ |
| Special functions (erf, erfc, gamma) | ✅ |
| Type conversion (double, char, integer types) | ✅ |
| Execution (eval, evalc) | ✅ |
| File system (pwd, ls, dir) | ✅ |
| Graphics stubs (plot, figure, subplot, etc.) | ✅ |
| 220+ built-in functions | ✅ |
| LSP server (completions, diagnostics, hover) | ✅ |
| Conformance suite (`conformance/`, 15 cases) | ✅ |
| MATBOX extensions (`#`, `**`, `endfunction`, `"strings"`) | ✅ |
| Block comments `%{...%}` | ✅ |
| Graphics engine | 🚧 In progress |
| Simulink solver | 🚧 In progress |
| Python bindings | 🚧 In progress |
| Tauri desktop app | 🚧 In progress |
| Web (TypeScript) engine | 📋 Planned |

---


## Benchmarks

| Benchmark              | Workload                                                                                                                  |        Time |
| :--------------------- | :------------------------------------------------------------------------------------------------------------------------ | ----------: |
| Stress Test Suite      | Arithmetic, Matrix Ops, Matrix Multiply, Statistics, FFT, Polynomial, Sets, Cells, Structs, Functions, Recursion, Strings | **1.508 s** |
| Matrix Multiplication  | 10 × (500×500 × 500×500) matrix multiplications                                                                           | **6.254 s** |
| Large Numerical Stress | 1,000,000 `sin()` evaluations + 1000×1000 matrix operations + FFT(4096) + Statistics + User Functions                     | **5.437 s** |

## CLI

| Command                    | Description                                 |
| :------------------------- | :------------------------------------------ |
| `matbox repl`              | Start interactive REPL                      |
| `matbox run <file.mbox>`   | Execute a MATBOX script                     |
| `matbox check <file.mbox>` | Check syntax without execution              |
| `matbox lsp`               | Start Language Server Protocol (LSP) server |
| `matbox version`           | Print version information                   |



## Benchmark Environment

| Property | Value         |
| :------- | :------------ |
| Version  | MATBOX v0.1.0 |
| Language | Rust          |
| Build    | Release       |
| Platform | Linux         |
| CPU      | *AMD Ryzen 7 7700*  |
| RAM      | *32GB DDR5*  |


## Development

### Running tests

```bash
cargo test                  # all tests (478)
cargo test -p matbox-core   # core engine tests
cargo test -p matbox-math   # math tests
cargo test -p matbox-cli    # conformance suite
```

### Conformance suite

`conformance/cases/*.mbox` holds one program per documented behavior, with the
expected stdout recorded in `conformance/expected/*.txt`. The suite runs under
`cargo test -p matbox-cli`, so CI fails if any behavior drifts.

When a change is meant to alter recorded behavior, regenerate the expected
files and read the diff before committing:

```bash
./scripts/update-expected.sh
```

### Code style

```bash
cargo fmt
cargo clippy
```

### Adding a builtin

Builtins are defined in `crates/matbox-core/src/builtins.rs`.
Each builtin is a match arm in the `call()` function. Add the function name
and implementation, then add tests in `crates/matbox-core/src/eval.rs`.

---

# Disclaimer

MATBOX is an independent, open-source project developed to provide a MATLAB-compatible programming language and runtime. It is currently under active development and should be considered **experimental software**.

## Project Status

MATBOX is **not feature-complete**. While many language features and built-in functions are implemented, others are still under development.

Users may encounter:

* Bugs or unexpected behavior
* Incomplete or missing language features
* Compatibility differences with MATLAB
* Performance limitations in some workloads
* Breaking changes between releases

Please verify important computational results before relying on MATBOX for research, engineering, academic, or production work.

## Compatibility

MATBOX aims to provide compatibility with MATLAB `.m` scripts wherever practical. However, compatibility is an ongoing effort, and not all MATLAB language features or built-in functions are currently supported.

Compatibility with MATLAB does **not** imply that MATBOX contains, copies, or redistributes any proprietary MATLAB code.

## Clean-Room Implementation

MATBOX is a **clean-room implementation** developed independently. It is not based on copied, decompiled, reverse-engineered, or otherwise derived proprietary MATLAB source code.

The project is implemented from original code using publicly available language behavior and documentation.

## Copyright & Trademarks

MATBOX is an independent project and is **not affiliated with, endorsed by, sponsored by, or associated with MathWorks**.

**MATLAB®** is a registered trademark of MathWorks. All other trademarks, product names, and company names mentioned in this project belong to their respective owners.

Copyright © 2026 Irfan. All rights reserved unless otherwise specified by the project's license.

## Open Source

MATBOX is distributed under the license included in this repository. Please read the LICENSE file for your rights and obligations regarding use, modification, and redistribution.

## No Warranty

THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, AND NON-INFRINGEMENT.

IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES, OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT, OR OTHERWISE, ARISING FROM, OUT OF, OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## Feedback

Bug reports, feature requests, pull requests, and community contributions are welcome and greatly appreciated. MATBOX is an evolving project, and community feedback helps improve its quality and compatibility.

# MATBOX Trademark Policy

The name **MATBOX™**, the MATBOX logo, and the project's branding are the intellectual property of the MATBOX project owner.

You are welcome to use, modify, and redistribute the MATBOX source code under the terms of the project's software license.

However, you **may not**:

* Redistribute a modified version under the name **MATBOX**.
* Use the **MATBOX** name or logo in a way that suggests official endorsement.
* Market or sell a fork as the official MATBOX project.

If you create a modified or forked version, you must use a different project name and clearly indicate that it is an unofficial fork.

This policy is intended to protect users from confusion and preserve the identity of the official MATBOX project.

The source code is licensed under AGPL-3.0, but the MATBOX name and logo may not be used for modified or redistributed versions without permission. See `TRADEMARKS.md` for details.

## License

### AGPLv3
