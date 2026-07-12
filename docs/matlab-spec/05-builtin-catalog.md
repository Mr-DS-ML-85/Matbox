# MATBOX Spec 05 — MATLAB Built-in Function Library Catalog

> **Clean-room note.** This catalog documents the *publicly documented behavior* of
> MATLAB built-in functions, described in our own words for a from-scratch Rust
> reimplementation. It is compiled from the public MathWorks "Functions by Category"
> reference index. No MathWorks source code was consulted or reproduced.

## How to read this catalog

- **Signature(s):** the common calling forms. `[a,b] = f(...)` denotes multiple outputs.
- **Semantics:** one-line documented behavior.
- **Notes:** `nargout`-dependent behavior, dimension conventions, edge cases, and other
  non-obvious details relevant to implementation.
- **Priority tags** in the Notes column:
  - **[CORE]** — must-have for a usable MATLAB-language runtime.
  - **[STD]** — standard library, expected but not blocking.
  - **[NICE]** — nice-to-have / specialized; can defer.

### General conventions that apply throughout

- Most element-wise math functions accept arrays of any size and apply element-wise,
  preserving shape. Empty inputs generally return empty of the same shape.
- Reduction functions (`sum`, `max`, `mean`, …) operate along the **first non-singleton
  dimension** by default, or along dimension `dim` when given. Many accept `'all'`
  (R2018b+) to reduce over every element, and a vector of dims (R2018b+).
- Indices returned by MATLAB functions are **1-based**.
- Many functions are **`nargout`-sensitive**: e.g. `max` returns just the value with one
  output but also the index with two; `size` returns a row vector with one output but
  individual dimension sizes with several.
- `NaN` propagation and the `'omitnan'` / `'includenan'` flag appear across the
  statistics and reduction families (R2015a+).

Sources (MathWorks "Functions by Category" index pages):
- Elementary math: <https://www.mathworks.com/help/matlab/elementary-math.html>
- Arithmetic: <https://www.mathworks.com/help/matlab/arithmetic-operations.html> ·
  Trigonometry: <https://www.mathworks.com/help/matlab/trigonometry.html> ·
  Exponents/logs: <https://www.mathworks.com/help/matlab/exponents-and-logarithms.html> ·
  Discrete math: <https://www.mathworks.com/help/matlab/discrete-math.html>
- Special functions: <https://www.mathworks.com/help/matlab/special-functions.html>
- Matrices & arrays: <https://www.mathworks.com/help/matlab/matrices-and-arrays.html>
- Linear algebra: <https://www.mathworks.com/help/matlab/linear-algebra.html>
- Set operations: <https://www.mathworks.com/help/matlab/set-operations.html> ·
  Logical operations: <https://www.mathworks.com/help/matlab/logical-operations.html>
- Strings: <https://www.mathworks.com/help/matlab/characters-and-strings.html>
- Data type conversion: <https://www.mathworks.com/help/matlab/data-type-conversion.html>
- Descriptive statistics: <https://www.mathworks.com/help/matlab/descriptive-statistics.html>
- Polynomials: <https://www.mathworks.com/help/matlab/polynomials.html> ·
  Interpolation: <https://www.mathworks.com/help/matlab/interpolation.html> ·
  Fourier/filtering: <https://www.mathworks.com/help/matlab/fourier-analysis-and-filtering.html>

---

## 1. Elementary Math

### 1.1 Arithmetic & rounding

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `plus` | `C = plus(A,B)`, `A+B` | Element-wise addition | Implicit expansion (broadcasting) since R2016b. **[CORE]** |
| `minus` | `A-B` | Element-wise subtraction | Broadcasting. **[CORE]** |
| `uminus` | `-A` | Unary negation | **[CORE]** |
| `uplus` | `+A` | Unary plus (no-op) | **[CORE]** |
| `times` | `A.*B` | Element-wise multiply | Broadcasting. **[CORE]** |
| `mtimes` | `A*B` | Matrix multiply | See Linear Algebra. **[CORE]** |
| `rdivide` | `A./B` | Element-wise right divide | **[CORE]** |
| `ldivide` | `A.\B` | Element-wise left divide (`B./A`) | **[CORE]** |
| `power` | `A.^B` | Element-wise power | Complex result when negative base, fractional exp. **[CORE]** |
| `mpower` | `A^B` | Matrix power | Integer power = repeated `mtimes`; general via eig. **[STD]** |
| `mod` | `mod(a,m)` | Modulo, result has sign of divisor `m` | `mod(a,0)=a`. **[CORE]** |
| `rem` | `rem(a,b)` | Remainder, result has sign of dividend `a` | `rem(a,0)=NaN`. **[CORE]** |
| `abs` | `abs(x)` | Absolute value / complex magnitude | **[CORE]** |
| `sign` | `sign(x)` | −1/0/1 by sign; `x/abs(x)` for complex | **[CORE]** |
| `ceil` | `ceil(x)` | Round toward +∞ | Also `ceil(t,unit)` for durations. **[CORE]** |
| `floor` | `floor(x)` | Round toward −∞ | **[CORE]** |
| `round` | `round(x)`, `round(x,n)`, `round(x,n,'significant')` | Round to nearest; ties away from zero | `n` = decimal digits. **[CORE]** |
| `fix` | `fix(x)` | Round toward zero (truncate) | **[CORE]** |
| `idivide` | `idivide(A,B,'fix')` | Integer division of integer types with rounding option | Options `fix`/`round`/`floor`/`ceil`. **[STD]** |
| `hypot` | `hypot(a,b)` | `sqrt(a.^2+b.^2)` avoiding overflow | **[STD]** |
| `cumsum` | `cumsum(A,dim)` | Cumulative sum | See §4. **[CORE]** |

### 1.2 Trigonometry

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `sin` `cos` `tan` | `sin(x)` | Trig of radians | **[CORE]** |
| `asin` `acos` `atan` | `asin(x)` | Inverse trig, radians | **[CORE]** |
| `atan2` | `atan2(y,x)` | Four-quadrant inverse tangent, radians | **[CORE]** |
| `atan2d` | `atan2d(y,x)` | Four-quadrant inverse tangent, degrees | **[STD]** |
| `sind` `cosd` `tand` | `sind(x)` | Trig of degrees (exact at multiples of 90) | **[STD]** |
| `asind` `acosd` `atand` | | Inverse trig, degrees | **[STD]** |
| `sinpi` `cospi` | `sinpi(x)` | `sin(pi*x)`/`cos(pi*x)` accurately | **[NICE]** |
| `sinh` `cosh` `tanh` | `sinh(x)` | Hyperbolic | **[STD]** |
| `asinh` `acosh` `atanh` | | Inverse hyperbolic | **[STD]** |
| `sec` `csc` `cot` (+`d`,`h`,`a…`) | `sec(x)` | Reciprocal trig family and their degree/hyperbolic/inverse variants | Full family: sec/csc/cot × {plain, d, h} × {a-prefixed}. **[NICE]** |
| `deg2rad` `rad2deg` | `deg2rad(x)` | Angle unit conversion | **[STD]** |
| `hypot` | `hypot(a,b)` | Hypotenuse (see §1.1) | **[STD]** |
| `cart2pol` `pol2cart` | `[th,r]=cart2pol(x,y)` | Cartesian↔polar/cylindrical | **[NICE]** |
| `cart2sph` `sph2cart` | | Cartesian↔spherical | **[NICE]** |

### 1.3 Exponents & logarithms

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `exp` | `exp(x)` | e^x element-wise | **[CORE]** |
| `expm1` | `expm1(x)` | `exp(x)-1` accurate for small x | **[NICE]** |
| `log` | `log(x)` | Natural log; complex for negative | **[CORE]** |
| `log2` | `log2(x)` or `[f,e]=log2(x)` | Base-2 log | Two-output form returns mantissa `f∈[0.5,1)` and exponent `e` s.t. `x=f*2^e`. **[STD]** |
| `log10` | `log10(x)` | Base-10 log | **[CORE]** |
| `log1p` | `log1p(x)` | `log(1+x)` accurate for small x | **[NICE]** |
| `sqrt` | `sqrt(x)` | Square root; complex for negative | **[CORE]** |
| `nthroot` | `nthroot(x,n)` | Real n-th root of real x | Handles negative x for odd n. **[STD]** |
| `pow2` | `pow2(x)`, `pow2(f,e)` | `2.^x`, or `f.*2.^e` | **[NICE]** |
| `nextpow2` | `nextpow2(n)` | Smallest p with `2^p >= abs(n)` | **[STD]** |
| `reallog` `realpow` `realsqrt` | `realsqrt(x)` | Real-only variants; error if complex would result | **[NICE]** |

### 1.4 Complex numbers

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `complex` | `complex(a,b)`, `complex(a)` | Construct complex from real/imag | Forces complex storage even if imag=0. **[CORE]** |
| `real` | `real(z)` | Real part | **[CORE]** |
| `imag` | `imag(z)` | Imaginary part | **[CORE]** |
| `conj` | `conj(z)` | Complex conjugate | **[CORE]** |
| `abs` | `abs(z)` | Magnitude | **[CORE]** |
| `angle` | `angle(z)` | Phase in radians (−π,π] | **[CORE]** |
| `arg` | — | (not a MATLAB name; use `angle`) | |
| `sign` | `sign(z)` | `z./abs(z)` for complex | **[STD]** |
| `unwrap` | `unwrap(p)` | Remove ±2π phase jumps | **[STD]** |
| `isreal` | `isreal(z)` | True if no complex storage | Note: `complex(1,0)` is *not* real. **[STD]** |
| `i` `j` | `i`, `1i` | Imaginary unit | Prefer `1i`/`1j` literals. **[CORE]** |

### 1.5 Discrete math

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `factorial` | `factorial(n)` | n! element-wise | Exact only up to n=170 (double); n≥171 → Inf. **[STD]** |
| `nchoosek` | `nchoosek(n,k)`, `nchoosek(v,k)` | Binomial coefficient, or all k-combinations of vector `v` | Scalar-n form → count; vector-v form → matrix of combos (rows). **[STD]** |
| `perms` | `perms(v)` | All permutations of `v` | Rows in reverse-lexicographic order; grows as n!. **[NICE]** |
| `factor` | `factor(n)` | Prime factors (ascending) | **[STD]** |
| `primes` | `primes(n)` | Primes ≤ n | **[STD]** |
| `isprime` | `isprime(A)` | Logical: which elements are prime | **[STD]** |
| `gcd` | `[g,c,d]=gcd(a,b)` | Greatest common divisor | Extra outputs give Bézout coefficients: `a*c+b*d=g`. **[STD]** |
| `lcm` | `lcm(a,b)` | Least common multiple | **[STD]** |
| `rat` | `[n,d]=rat(x)`, `s=rat(x)` | Rational approximation | One output → char form `"1 + 1/3"`; two → numerator/denominator. **[NICE]** |
| `rats` | `rats(x)` | Rational as text | **[NICE]** |
| `mod` `rem` | | (see §1.1) | **[CORE]** |
| `matchpairs` | `matchpairs(cost,unmatched)` | Linear assignment (min cost) | **[NICE]** |

---

## 2. Special Functions

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `erf` | `erf(x)` | Error function | **[STD]** |
| `erfc` | `erfc(x)` | Complementary error function `1-erf` | Accurate in tail. **[STD]** |
| `erfcx` | `erfcx(x)` | Scaled `exp(x^2)*erfc(x)` | **[NICE]** |
| `erfinv` | `erfinv(x)` | Inverse error function | **[STD]** |
| `erfcinv` | `erfcinv(x)` | Inverse complementary erf | **[NICE]** |
| `gamma` | `gamma(x)` | Gamma function | **[STD]** |
| `gammaln` | `gammaln(x)` | log Γ(x) (avoids overflow) | **[STD]** |
| `gammainc` | `gammainc(x,a)`, `gammainc(x,a,'upper')` | Regularized incomplete gamma | Lower by default. **[NICE]** |
| `gammaincinv` | `gammaincinv(y,a)` | Inverse incomplete gamma | **[NICE]** |
| `psi` | `psi(x)`, `psi(k,x)` | Digamma / k-th polygamma | **[NICE]** |
| `beta` | `beta(z,w)` | Beta function Γ(z)Γ(w)/Γ(z+w) | **[STD]** |
| `betaln` | `betaln(z,w)` | log beta | **[NICE]** |
| `betainc` | `betainc(x,z,w)` | Incomplete beta | **[NICE]** |
| `betaincinv` | `betaincinv(y,z,w)` | Inverse incomplete beta | **[NICE]** |
| `besselj` | `besselj(nu,z)` | Bessel 1st kind | **[NICE]** |
| `bessely` | `bessely(nu,z)` | Bessel 2nd kind | **[NICE]** |
| `besseli` `besselk` | `besseli(nu,z)` | Modified Bessel 1st/2nd kind | **[NICE]** |
| `besselh` | `besselh(nu,K,z)` | Hankel (Bessel 3rd kind) | **[NICE]** |
| `airy` | `airy(k,z)` | Airy functions & derivatives | **[NICE]** |
| `legendre` | `legendre(n,x)` | Associated Legendre P_n^m | **[NICE]** |
| `ellipke` | `[K,E]=ellipke(m)` | Complete elliptic integrals 1st/2nd | **[NICE]** |
| `ellipj` | `[sn,cn,dn]=ellipj(u,m)` | Jacobi elliptic functions | **[NICE]** |
| `expint` | `expint(x)` | Exponential integral E₁ | **[NICE]** |

---

## 3. Array Creation

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `zeros` | `zeros(n)`, `zeros(m,n)`, `zeros(sz)`, `zeros(...,'like',p)`, `zeros(...,classname)` | All-zeros array | `zeros(n)` is n×n. Class arg e.g. `'int8'`,`'single'`. **[CORE]** |
| `ones` | `ones(...)` | All-ones array | Same forms as `zeros`. **[CORE]** |
| `eye` | `eye(n)`, `eye(m,n)`, `eye(sz,classname)` | Identity (1 on main diagonal) | Non-square allowed. **[CORE]** |
| `true` | `true`, `true(n)`, `true(sz)` | Logical-1 array | **[CORE]** |
| `false` | `false(...)` | Logical-0 array | **[CORE]** |
| `nan` / `NaN` | `nan(n)`, `NaN` | Not-a-Number array | Also `nan('single')`. **[CORE]** |
| `inf` / `Inf` | `inf(n)`, `Inf` | Infinity array | **[CORE]** |
| `rand` | `rand(...)`, `rand(...,'like',p)` | Uniform (0,1) random | Uses global RNG stream (`rng`). **[CORE]** |
| `randn` | `randn(...)` | Standard-normal random | **[CORE]** |
| `randi` | `randi(imax,...)`, `randi([lo hi],...)` | Uniform random integers | Default class `double`; can request e.g. `'int32'`. **[CORE]** |
| `randperm` | `randperm(n)`, `randperm(n,k)` | Random permutation / k of them | **[STD]** |
| `linspace` | `linspace(a,b)`, `linspace(a,b,n)` | n linearly spaced points inclusive | Default n=100; always includes both ends. **[CORE]** |
| `logspace` | `logspace(a,b)`, `logspace(a,b,n)` | n points from 10^a to 10^b | Default n=50. **[STD]** |
| `colon` | `colon(a,b)`, `colon(a,d,b)`, `a:b`, `a:d:b` | Range vector | Empty if step direction wrong; float step accumulates carefully. **[CORE]** |
| `magic` | `magic(n)` | n×n magic square | **[NICE]** |
| `diag` | `diag(v)`, `diag(A)`, `diag(v,k)`, `diag(A,k)` | Build diagonal matrix from vector, or extract diagonal from matrix | Behavior flips on vector vs matrix input; `k` selects super/sub-diagonal. **[CORE]** |
| `blkdiag` | `blkdiag(A,B,...)` | Block-diagonal assembly | **[STD]** |
| `cat` | `cat(dim,A,B,...)` | Concatenate along dim | Basis of horzcat/vertcat. **[CORE]** |
| `horzcat` | `horzcat(A,B)`, `[A,B]` | Concatenate columns (dim 2) | **[CORE]** |
| `vertcat` | `vertcat(A,B)`, `[A;B]` | Concatenate rows (dim 1) | **[CORE]** |
| `repmat` | `repmat(A,n)`, `repmat(A,m,n)`, `repmat(A,sz)` | Tile copies of array | **[CORE]** |
| `repelem` | `repelem(v,n)`, `repelem(A,r,c)` | Repeat individual elements | Distinct from `repmat`. **[STD]** |
| `meshgrid` | `[X,Y]=meshgrid(x,y)`, `[X,Y,Z]=meshgrid(x,y,z)` | 2-D/3-D grid, x varies across columns | Row/col convention opposite `ndgrid`. **[STD]** |
| `ndgrid` | `[X1,...]=ndgrid(x1,...)` | N-D grid, first dim varies down rows | **[STD]** |
| `freqspace` | `freqspace(n)` | Frequency-response spacing | **[NICE]** |
| `combinations` | `combinations(v1,v2,...)` | All element combinations as a table | R2023a+. **[NICE]** |

---

## 4. Array Manipulation, Reduction & Sorting

### 4.1 Size / shape / order

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `size` | `sz=size(A)`, `[m,n]=size(A)`, `size(A,dim)`, `[m,n,p]=size(A)`, `size(A,[d1 d2])` | Array dimensions | **nargout-sensitive**: 1 output → row vector; last of k outputs collapses remaining dims (product). `dim` may be vector (R2019b+). **[CORE]** |
| `length` | `length(A)` | Largest dimension size; `max(size(A))`, 0 if empty | **[CORE]** |
| `numel` | `numel(A)` | Total element count | Also used to overload indexing count. **[CORE]** |
| `ndims` | `ndims(A)` | Number of dimensions (≥2) | **[CORE]** |
| `isempty` | `isempty(A)` | True if any dim is 0 | **[CORE]** |
| `isscalar` `isvector` `ismatrix` `isrow` `iscolumn` | `isvector(A)` | Shape predicates | **[STD]** |
| `issorted` | `issorted(A)`, `issorted(A,...,'descend')` | Test sorted order | **[STD]** |
| `issortedrows` | `issortedrows(A)` | Test row-sorted | **[NICE]** |

### 4.2 Reshape / rearrange

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `reshape` | `reshape(A,m,n)`, `reshape(A,sz)`, `reshape(A,m,[],p)` | Reinterpret shape, column-major order | One dim may be `[]` (inferred); element count must match. **[CORE]** |
| `permute` | `permute(A,order)` | Generalized transpose over dims | **[CORE]** |
| `ipermute` | `ipermute(A,order)` | Inverse of `permute` | **[STD]** |
| `squeeze` | `squeeze(A)` | Remove singleton dims | Never reduces below 2-D. **[STD]** |
| `shiftdim` | `[B,n]=shiftdim(A)`, `shiftdim(A,n)` | Shift dims, or drop leading singletons | **[NICE]** |
| `transpose` | `A.'`, `transpose(A)` | Non-conjugate transpose | **[CORE]** |
| `ctranspose` | `A'`, `ctranspose(A)` | Complex-conjugate transpose | **[CORE]** |
| `circshift` | `circshift(A,k)`, `circshift(A,k,dim)` | Circular shift | Vector shifts first non-singleton dim. **[STD]** |
| `flip` | `flip(A)`, `flip(A,dim)` | Reverse element order along dim | **[STD]** |
| `fliplr` | `fliplr(A)` | Flip left-right (dim 2) | **[STD]** |
| `flipud` | `flipud(A)` | Flip up-down (dim 1) | **[STD]** |
| `rot90` | `rot90(A)`, `rot90(A,k)` | Rotate 90° k times, counterclockwise | **[STD]** |
| `kron` | `kron(A,B)` | Kronecker tensor product | **[STD]** |
| `cat`/`horzcat`/`vertcat`/`repmat`/`repelem` | | (see §3) | |
| `tril` `triu` | `tril(A,k)` | Lower/upper triangular part | **[STD]** |

### 4.3 Sorting & set-like

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `sort` | `[B,I]=sort(A)`, `sort(A,dim)`, `sort(A,dir)`, `sort(...,'ComparisonMethod',...)` | Sort along dim | 2nd output = permutation indices. Default ascending; NaNs sort to end. Stable for equal keys. **[CORE]** |
| `sortrows` | `[B,I]=sortrows(A)`, `sortrows(A,col)` | Sort rows as tuples | `col` negative → descending on that column. **[STD]** |
| `unique` | `[C,ia,ic]=unique(A)`, `unique(A,'rows')`, `unique(A,'stable')` | Unique values, sorted by default | `ia`: indices into A giving C; `ic`: map A→C. `'last'`/`'first'` choose which duplicate index. **[CORE]** |
| `find` | `find(X)`, `find(X,k)`, `find(X,k,'last')`, `[r,c]=find(X)`, `[r,c,v]=find(X)` | Indices of nonzeros | 1 output → linear indices; 2 → row/col subs; 3 → also values. `k` limits count. **[CORE]** |
| `any` | `any(X)`, `any(X,dim)`, `any(X,'all')` | True if any nonzero along dim | Reduces first non-singleton dim by default. **[CORE]** |
| `all` | `all(X)`, `all(X,dim)` | True if all nonzero | Empty → true (vacuous). **[CORE]** |
| `ismember` | `ismember(A,B)`, `[tf,loc]=ismember(A,B)`, `ismember(A,B,'rows')` | Membership test | `loc` = lowest index in B, 0 if absent. **[CORE]** |

### 4.4 Cumulative / differences

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `cumsum` | `cumsum(A)`, `cumsum(A,dim)`, `cumsum(...,'reverse')`, `...,'omitnan'` | Cumulative sum | **[CORE]** |
| `cumprod` | `cumprod(A,dim)` | Cumulative product | **[STD]** |
| `cummax` `cummin` | `cummax(A)` | Cumulative max/min | **[NICE]** |
| `diff` | `diff(A)`, `diff(A,n)`, `diff(A,n,dim)` | n-th order differences | Output shrinks by n along dim. **[CORE]** |
| `movsum` `movmean` `movmedian` `movstd` `movvar` `movmax` `movmin` `movprod` | `movmean(A,k)`, `movmean(A,[kb kf])` | Sliding-window statistics | Window `k` centered; endpoints shrink unless `'Endpoints'` set. **[NICE]** |

### 4.5 Reductions & descriptive statistics

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `sum` | `sum(A)`, `sum(A,dim)`, `sum(A,'all')`, `sum(...,'omitnan')`, `sum(A,dim,type)` | Sum along dim | `type` `'double'`/`'native'`. Empty sum = 0. **[CORE]** |
| `prod` | `prod(A,dim)` | Product along dim | Empty product = 1. **[CORE]** |
| `max` | `[M,I]=max(A)`, `max(A,[],dim)`, `max(A,B)`, `max(A,[],'all')`, `max(A,[],...,'ComparisonMethod',...)`, `maxk` | Maximum | **nargout**: 2nd output = index (first occurrence). Two-array form is element-wise. Note the `[]` placeholder to pass `dim`. NaNs ignored by default (`'omitnan'`). **[CORE]** |
| `min` | analogous to `max` | Minimum | **[CORE]** |
| `maxk` `mink` | `maxk(A,k)`, `[v,i]=maxk(A,k)` | k largest / smallest | Sorted by magnitude of value. **[NICE]** |
| `bounds` | `[lo,hi]=bounds(A)` | Min and max together | **[NICE]** |
| `mean` | `mean(A,dim)`, `mean(A,'all')`, `mean(...,'omitnan')` | Arithmetic mean | **[CORE]** |
| `median` | `median(A,dim)` | Median | Average of two middle for even count. **[STD]** |
| `mode` | `[m,f,c]=mode(A)` | Most frequent value | Smallest value on ties; extra outputs = count, cell of all modes. **[STD]** |
| `std` | `std(A)`, `std(A,w)`, `std(A,w,dim)` | Standard deviation | `w=0` (default) → N−1 normalization; `w=1` → N. **[STD]** |
| `var` | `var(A,w,dim)` | Variance | Same weight convention as `std`. **[STD]** |
| `range` | `range(A)` | max−min (Statistics TB) | **[NICE]** (toolbox) |
| `rms` | `rms(A)` | Root mean square | **[NICE]** |
| `prctile` `quantile` `iqr` | `prctile(A,p)` | Percentiles / quantiles / IQR | Statistics TB. **[NICE]** |
| `histcounts` | `[N,edges]=histcounts(X)`, `histcounts(X,edges)`, `histcounts(X,nbins)`, `[N,edges,bin]=histcounts(X)` | Bin counts (histogram data) | Auto bin selection; 3rd output = bin index per element. **[STD]** |
| `histc` | `histc(X,edges)` | Legacy bin counts | Superseded by `histcounts`. **[NICE]** |
| `discretize` | `[bin,edges]=discretize(X,edges)` | Assign data to bins | **[NICE]** |
| `accumarray` | `accumarray(subs,val)`, `accumarray(subs,val,sz,fun,fill)` | Accumulate values by subscript | Powerful group-reduce primitive. **[STD]** |

---

## 5. Linear Algebra

### 5.1 Solving & inverses

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `mldivide` | `A\B`, `mldivide(A,B)` | Solve `A*x=B` | Square → LU solve; rectangular → least-squares/min-norm via QR; triangular/other special structures detected. **[CORE]** |
| `mrdivide` | `B/A`, `mrdivide(B,A)` | Solve `x*A=B` | Equivalent to `(A'\B')'`. **[CORE]** |
| `inv` | `inv(A)` | Matrix inverse | Warns near-singular; prefer `\`. **[CORE]** |
| `pinv` | `pinv(A)`, `pinv(A,tol)` | Moore–Penrose pseudoinverse | Via SVD. **[STD]** |
| `linsolve` | `linsolve(A,B)`, `linsolve(A,B,opts)`, `[x,r]=linsolve(...)` | Solve with structure options | `opts` struct declares symmetry/triangular etc.; 2nd output = reciprocal cond / rank. **[STD]** |
| `decomposition` | `dA=decomposition(A)` then `dA\B` | Reusable factorization object | Amortizes cost over multiple RHS. **[NICE]** |
| `lsqminnorm` | `lsqminnorm(A,B)` | Min-norm least-squares | **[NICE]** |
| `sylvester` | `sylvester(A,B,C)` | Solve `AX+XB=C` | **[NICE]** |

### 5.2 Scalars / properties

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `det` | `det(A)` | Determinant | Via LU; may overflow — prefer `logdet` idioms. **[STD]** |
| `rank` | `rank(A)`, `rank(A,tol)` | Numerical rank via SVD | **[STD]** |
| `trace` | `trace(A)` | Sum of diagonal | **[STD]** |
| `norm` | `norm(v)`, `norm(v,p)`, `norm(A)`, `norm(A,1)`, `norm(A,inf)`, `norm(A,'fro')` | Vector/matrix norm | Vector default p=2; matrix default = 2-norm (largest sing. value). **[CORE]** |
| `vecnorm` | `vecnorm(A,p,dim)` | Column/row-wise vector norm | **[STD]** |
| `normest` | `normest(A)` | Estimate 2-norm | **[NICE]** |
| `cond` | `cond(A)`, `cond(A,p)` | Condition number | **[STD]** |
| `rcond` `condest` | `rcond(A)` | Reciprocal / 1-norm cond estimate | **[NICE]** |
| `null` | `null(A)`, `null(A,'r')` | Orthonormal null-space basis | `'r'` gives rational basis. **[STD]** |
| `orth` | `orth(A)` | Orthonormal range basis | **[STD]** |
| `rref` | `[R,p]=rref(A)` | Reduced row echelon form | 2nd output = pivot columns. **[STD]** |
| `subspace` | `subspace(A,B)` | Angle between subspaces | **[NICE]** |

### 5.3 Decompositions & eigen/SVD

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `eig` | `e=eig(A)`, `[V,D]=eig(A)`, `eig(A,B)`, `[V,D,W]=eig(A)`, `eig(A,'balance'/'nobalance')` | Eigenvalues/vectors | **nargout**: 1 → column of eigenvalues; 2 → `A*V=V*D`; 3 → left eigenvectors `W`. Generalized form with `B`. **[STD]** |
| `svd` | `s=svd(A)`, `[U,S,V]=svd(A)`, `svd(A,'econ')`, `svd(A,0)` | Singular value decomposition | 1 output → singular values vector; 3 → `A=U*S*V'`. `'econ'` for thin. **[STD]** |
| `lu` | `[L,U]=lu(A)`, `[L,U,P]=lu(A)`, `[L,U,P,Q]=lu(S)` | LU factorization with pivoting | 2-output form folds permutation into `L` (psychologically lower triangular). **[STD]** |
| `qr` | `[Q,R]=qr(A)`, `[Q,R,P]=qr(A)`, `qr(A,0)`/`'econ'`, `R=qr(A)` | QR factorization | Column pivoting with 3 outputs; economy form for tall A. **[STD]** |
| `chol` | `R=chol(A)`, `[R,p]=chol(A)`, `chol(A,'lower')` | Cholesky (needs SPD) | `R'*R=A` (upper by default). 2nd output `p=0` iff SPD (no error). **[STD]** |
| `ldl` | `[L,D,P]=ldl(A)` | LDLᵀ for symmetric indefinite | **[NICE]** |
| `schur` | `[U,T]=schur(A)`, `schur(A,'complex')` | Schur decomposition | **[NICE]** |
| `hess` | `[P,H]=hess(A)` | Hessenberg form | **[NICE]** |
| `qz` | `[AA,BB,Q,Z,V,W]=qz(A,B)` | Generalized Schur | **[NICE]** |
| `gsvd` `polyeig` `ordeig` `ordschur` `balance` | | Specialized eigen/SVD tools | **[NICE]** |

### 5.4 Matrix functions & products

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `expm` | `expm(A)` | Matrix exponential (not element-wise) | Contrast `exp`. **[STD]** |
| `logm` | `[L,e]=logm(A)` | Matrix logarithm | **[NICE]** |
| `sqrtm` | `[X,r]=sqrtm(A)` | Matrix square root | **[NICE]** |
| `funm` | `funm(A,@fun)` | General matrix function | **[NICE]** |
| `mpower` | `A^p` | Matrix power | (see §1). **[STD]** |
| `kron` | `kron(A,B)` | Kronecker product | **[STD]** |
| `cross` | `cross(a,b)`, `cross(a,b,dim)` | Vector cross product | Operates on length-3 vectors. **[STD]** |
| `dot` | `dot(a,b)`, `dot(a,b,dim)` | Scalar/inner product | Conjugates first arg for complex. **[STD]** |
| `bandwidth` `isbanded` `isdiag` `issymmetric` `ishermitian` `istriu` `istril` | `issymmetric(A)` | Structure predicates | **[STD]** |
| `pagemtimes` `pagetranspose` `pageinv` `pagemldivide` `pagesvd` `pageeig` ... | `pagemtimes(A,B)` | Page-wise (batched) matrix ops over 3-D+ arrays | R2020b+. **[NICE]** |

---

## 6. Relational, Set & Logical

### 6.1 Relational operators

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `eq` | `A==B` | Element-wise equality | Broadcasting; NaN≠NaN. **[CORE]** |
| `ne` | `A~=B` | Inequality | **[CORE]** |
| `lt` `gt` `le` `ge` | `A<B` `A>B` `A<=B` `A>=B` | Relational comparisons | Complex compared by real part then imag (for sort) — but relational ops use real part only. **[CORE]** |
| `isequal` | `isequal(A,B,...)` | Deep equality (NaN≠NaN) | Handles nested cells/structs; size-sensitive. **[CORE]** |
| `isequaln` | `isequaln(A,B)` | Like `isequal` but NaN==NaN | **[STD]** |

### 6.2 Logical operators & predicates

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `and` | `A & B` | Element-wise AND | **[CORE]** |
| `or` | `A \| B` | Element-wise OR | **[CORE]** |
| `not` | `~A` | Element-wise NOT | **[CORE]** |
| `xor` | `xor(A,B)` | Exclusive OR | **[STD]** |
| `&&` `\|\|` | `a && b` | Short-circuit AND/OR | Scalar operands only; control-flow semantics. **[CORE]** |
| `any` `all` | (see §4.3) | Reductions | **[CORE]** |
| `find` | (see §4.3) | Nonzero indices | **[CORE]** |
| `true` `false` | (see §3) | Logical constants | **[CORE]** |
| `logical` | `logical(A)` | Convert numeric→logical | Nonzero→true; errors on NaN. **[CORE]** |
| `islogical` | `islogical(A)` | Type test | **[STD]** |

### 6.3 Set operations

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `unique` | (see §4.3) | Unique set | **[CORE]** |
| `ismember` | (see §4.3) | Membership | **[CORE]** |
| `intersect` | `[C,ia,ib]=intersect(A,B)`, `intersect(A,B,'rows')`, `'stable'` | Set intersection | Sorted unique by default; index outputs into A and B. **[STD]** |
| `union` | `[C,ia,ib]=union(A,B)` | Set union | **[STD]** |
| `setdiff` | `[C,ia]=setdiff(A,B)` | Elements in A not in B | **[STD]** |
| `setxor` | `[C,ia,ib]=setxor(A,B)` | Symmetric difference | **[STD]** |
| `ismembertol` `uniquetol` | `uniquetol(A,tol)` | Tolerance-based variants | **[NICE]** |
| `allunique` `numunique` | `allunique(A)` | All-unique test / count | R2024a+. **[NICE]** |

---

## 7. Strings & Character Arrays

> MATLAB distinguishes **char arrays** (`'text'`), **string arrays** (`"text"`, R2016b+),
> and **cell arrays of char** (`{'a','b'}`). Most modern functions accept all three.

### 7.1 Create / convert

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `string` | `string(A)`, `""` | Build / convert to string array | Converts numbers, chars, cellstr. **[CORE]** |
| `char` | `char(A)`, `char(c1,c2,...)` | Build char array / convert codes↔chars | Multiple args pad to a char matrix. **[CORE]** |
| `cellstr` | `cellstr(A)` | Convert to cell array of char rows | **[STD]** |
| `strings` | `strings(n)` | Array of `""` | **[STD]** |
| `blanks` | `blanks(n)` | n-space char row | **[NICE]** |
| `newline` | `newline` | The `char(10)` newline | **[STD]** |
| `sprintf` | `str=sprintf(fmt,A,...)`, `[str,err]=sprintf(...)` | Format data to string | Recycles format over extra data; `%d %f %s %g %e %x %c` etc.; `*` for field width. Column-major consumption of args. **[CORE]** |
| `compose` | `compose(fmt,A)` | Like sprintf → string array (one per row/element) | **[STD]** |
| `strcat` | `strcat(s1,s2,...)` | Horizontal concatenation | For char inputs, trailing whitespace removed; for strings/cells it is preserved. **[STD]** |
| `append` | `append(s1,s2,...)` | Concatenate strings (keeps whitespace) | R2020b+. **[STD]** |
| `join` | `join(str)`, `join(str,delim)` | Join string array along dim with delimiter | **[STD]** |
| `num2str` | `num2str(A)`, `num2str(A,precision)`, `num2str(A,fmt)` | Number → char (for display) | Builds aligned char matrix for matrices. **[CORE]** |
| `str2num` | `str2num(s)` | Evaluate text as MATLAB expression → numeric | **Uses eval — security/robustness caveat**; returns `[]` on failure. **[STD]** |
| `str2double` | `str2double(s)` | Parse scalar number per element | `NaN` on unparseable; works on string/cell arrays element-wise. **[CORE]** |
| `int2str` | `int2str(A)` | Round and convert to integer text | **[STD]** |
| `mat2str` | `mat2str(A)`, `mat2str(A,n)` | Char that reproduces matrix via eval (with brackets) | **[STD]** |
| `sscanf` | `[A,n]=sscanf(str,fmt)` | Parse formatted text → numeric | Column-major fill. **[STD]** |
| `convertCharsToStrings` `convertStringsToChars` | | Interconvert char/string for arg handling | **[NICE]** |

### 7.2 Compare / search / test

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `strcmp` | `strcmp(a,b)` | Whole-string equality (case sensitive) | Returns logical; array-aware. **[CORE]** |
| `strcmpi` | `strcmpi(a,b)` | Case-insensitive equality | **[STD]** |
| `strncmp` `strncmpi` | `strncmp(a,b,n)` | Compare first n chars | **[STD]** |
| `contains` | `contains(s,pat)`, `contains(s,pat,'IgnoreCase',true)` | Substring/pattern test | **[CORE]** |
| `startsWith` `endsWith` | `startsWith(s,pat)` | Prefix/suffix test | **[STD]** |
| `matches` | `matches(s,pat)` | Whole-string pattern match | **[NICE]** |
| `count` | `count(s,pat)` | Count occurrences | **[STD]** |
| `strfind` | `strfind(s,sub)` | Start indices of all occurrences | Returns `[]` (or cell for cellstr) if none. **[CORE]** |
| `strtok` | `[tok,rem]=strtok(s)`, `strtok(s,delim)` | First token + remainder | **[STD]** |
| `strlength` | `strlength(s)` | Number of chars per string | Differs from `length` for string arrays. **[STD]** |
| `ischar` `isstring` `iscellstr` `isStringScalar` | `ischar(s)` | Type tests | **[STD]** |
| `isletter` `isspace` `isstrprop` | `isstrprop(s,'digit')` | Per-character classification | **[NICE]** |

### 7.3 Replace / edit / case

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `strrep` | `strrep(s,old,new)` | Replace all occurrences | **[CORE]** |
| `replace` | `replace(s,old,new)` | String-array replace | R2016b+, more general. **[STD]** |
| `replaceBetween` | `replaceBetween(s,a,b,new)` | Replace text between bounds | **[NICE]** |
| `erase` | `erase(s,pat)` | Delete matches | **[STD]** |
| `eraseBetween` | `eraseBetween(s,a,b)` | Delete between bounds | **[NICE]** |
| `insertAfter` `insertBefore` | `insertAfter(s,pat,txt)` | Insert relative to match | **[NICE]** |
| `extractAfter` `extractBefore` `extractBetween` | `extractBefore(s,pat)` | Extract substrings | **[STD]** |
| `extract` | `extract(s,pat)` | Extract all matches | **[NICE]** |
| `pad` | `pad(s,n)`, `pad(s,n,side,padchar)` | Pad to width | **[NICE]** |
| `strip` | `strip(s)`, `strip(s,side,ch)` | Trim leading/trailing chars | **[STD]** |
| `strtrim` | `strtrim(s)` | Remove leading/trailing whitespace | **[CORE]** |
| `deblank` | `deblank(s)` | Remove trailing whitespace | **[STD]** |
| `strjust` | `strjust(s,'right')` | Justify char matrix | **[NICE]** |
| `upper` | `upper(s)` | Uppercase | **[CORE]** |
| `lower` | `lower(s)` | Lowercase | **[CORE]** |
| `reverse` | `reverse(s)` | Reverse char order | **[NICE]** |

### 7.4 Split / join / regex

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `split` | `split(s)`, `split(s,delim)`, `[c,m]=split(...)` | Split string at delimiters → string array | Splits along a new dimension. **[STD]** |
| `strsplit` | `strsplit(s)`, `strsplit(s,delim)` | Split char row → cell of char | Default splits on whitespace. **[STD]** |
| `splitlines` | `splitlines(s)` | Split at newlines | **[NICE]** |
| `strjoin` | `strjoin(c)`, `strjoin(c,delim)` | Join cellstr with delimiter | **[STD]** |
| `regexp` | `regexp(s,expr)`, `regexp(s,expr,opt,...)`, `[tok,mat]=regexp(...)` | Regular-expression match | **Highly nargout/option driven**: option keywords `'match'`,`'tokens'`,`'start'`,`'end'`,`'names'`,`'split'`,`'once'` select outputs; default returns start indices. **[STD]** |
| `regexpi` | `regexpi(s,expr,...)` | Case-insensitive regexp | **[STD]** |
| `regexprep` | `regexprep(s,expr,repl)` | Regex search-and-replace | Supports `$1` backreferences, dynamic `${...}`. **[STD]** |
| `regexptranslate` | `regexptranslate('escape',s)` | Escape/convert to regex | **[NICE]** |
| `pattern` builders (`digitsPattern`,`lettersPattern`,`wildcardPattern`,`asManyOfPattern`, …) | | Composable pattern objects (alternative to regex) | R2020b+; large family. **[NICE]** |

---

## 8. Cell Arrays & Structures

### 8.1 Functional iteration

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `cellfun` | `cellfun(@f,C)`, `cellfun(@f,C1,C2)`, `cellfun(@f,C,'UniformOutput',false)`, `[a,b]=cellfun(...)` | Apply function to each cell | Default expects scalar outputs (uniform); set `UniformOutput=false` to collect into a cell. Multiple output collection supported. Some name-string forms (`cellfun('isempty',C)`) are optimized. **[STD]** |
| `arrayfun` | `arrayfun(@f,A)`, `arrayfun(@f,A,'UniformOutput',false)` | Apply function to each array element | Same uniform-output rules as `cellfun`. **[STD]** |
| `structfun` | `structfun(@f,S)`, `...,'UniformOutput',false` | Apply function to each field | Operates over scalar-struct fields; uniform → column vector. **[NICE]** |

### 8.2 Struct construction / field ops

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `struct` | `struct('f1',v1,'f2',v2,...)` | Build struct/array | Cell-array values with same size create a struct *array*. **[CORE]** |
| `fieldnames` | `fieldnames(S)` | Cell of field name strings | **[CORE]** |
| `isfield` | `isfield(S,name)`, `isfield(S,cellOfNames)` | Field-existence test | **[CORE]** |
| `rmfield` | `rmfield(S,name)` | Remove field(s) | **[STD]** |
| `setfield` | `setfield(S,name,val)` | Functional field set | Rarely used vs dot-assign. **[NICE]** |
| `getfield` | `getfield(S,name)` | Functional field get | **[NICE]** |
| `orderfields` | `[S,perm]=orderfields(S)` | Sort/reorder fields | **[NICE]** |
| `struct2cell` | `struct2cell(S)` | Field values → cell (drops names) | Names recoverable via `fieldnames`. **[STD]** |
| `cell2struct` | `cell2struct(C,names,dim)` | Cell + names → struct | **[STD]** |
| `struct2table` `table2struct` | | Struct↔table | **[NICE]** |
| `namedargs2cell` | `namedargs2cell(S)` | Struct → name/value cell (for arg forwarding) | **[NICE]** |
| `isstruct` | `isstruct(S)` | Type test | **[STD]** |

### 8.3 Cell construction / conversion

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `cell` | `cell(n)`, `cell(m,n)` | Preallocate empty cell array | Cells init to `[]`. **[CORE]** |
| `num2cell` | `num2cell(A)`, `num2cell(A,dims)` | Array → cell, one element per cell (or slices) | **[STD]** |
| `cell2mat` | `cell2mat(C)` | Concatenate cell contents into array | All cells must be compatible types/sizes. **[STD]** |
| `mat2cell` | `mat2cell(A,rowsizes,colsizes)` | Partition array into cell of subarrays | Size vectors must sum to each dim. **[STD]** |
| `iscell` | `iscell(C)` | Type test | **[STD]** |
| `deal` | `[a,b,c]=deal(x)`, `deal(x,y,z)` | Distribute values to multiple outputs | Common idiom for multi-assign. **[STD]** |

---

## 9. Data Analysis, Signal & Polynomials

### 9.1 Transforms & filtering

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `fft` | `fft(x)`, `fft(x,n)`, `fft(x,n,dim)` | Discrete Fourier transform | `n` pads/truncates; operates on columns for matrices (first non-singleton dim). **[STD]** |
| `ifft` | `ifft(y)`, `ifft(y,n,dim)`, `ifft(...,'symmetric')` | Inverse FFT | `'symmetric'` forces real output. **[STD]** |
| `fft2` `ifft2` `fftn` `ifftn` | `fft2(x)` | 2-D / N-D FFT | **[NICE]** |
| `fftshift` | `fftshift(x)`, `fftshift(x,dim)` | Center zero-frequency | **[STD]** |
| `ifftshift` | `ifftshift(x)` | Inverse of `fftshift` | **[NICE]** |
| `nextpow2` | (see §1.3) | FFT sizing helper | **[STD]** |
| `conv` | `conv(u,v)`, `conv(u,v,'same')`, `conv(u,v,'valid')` | 1-D convolution / polynomial multiply | Default `'full'` (len m+n−1). **[STD]** |
| `conv2` | `conv2(A,B,shape)` | 2-D convolution | **[NICE]** |
| `convn` | `convn(A,B)` | N-D convolution | **[NICE]** |
| `deconv` | `[q,r]=deconv(y,h)` | Deconvolution / polynomial division | Returns quotient and remainder. **[STD]** |
| `filter` | `filter(b,a,x)`, `[y,zf]=filter(b,a,x,zi)` | Rational (IIR/FIR) digital filter | `zf`/`zi` = filter state for streaming. **[STD]** |
| `filter2` | `filter2(H,X,shape)` | 2-D correlation-based filtering | **[NICE]** |
| `interpft` | `interpft(x,n)` | FFT-based resampling interpolation | **[NICE]** |

### 9.2 Polynomials

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `polyval` | `polyval(p,x)`, `[y,delta]=polyval(p,x,S)` | Evaluate polynomial (coeffs high→low power) | With `S` from `polyfit`, gives error estimate. **[STD]** |
| `polyfit` | `[p,S,mu]=polyfit(x,y,n)` | Least-squares polynomial fit of degree n | 3rd output `mu` for centering/scaling; use with `polyval`. **[STD]** |
| `roots` | `roots(p)` | Polynomial roots | Via companion-matrix eigenvalues. **[STD]** |
| `poly` | `poly(r)`, `poly(A)` | Coeffs from roots, or characteristic polynomial of matrix | **[STD]** |
| `conv` | (see §9.1) | Polynomial multiply | **[STD]** |
| `deconv` | (see §9.1) | Polynomial divide | **[STD]** |
| `polyder` | `polyder(p)`, `polyder(a,b)` | Derivative (or product-rule derivative) | **[STD]** |
| `polyint` | `polyint(p,k)` | Analytic integral with constant k | **[STD]** |
| `polyvalm` | `polyvalm(p,A)` | Matrix-argument polynomial evaluation | **[NICE]** |
| `residue` | `[r,p,k]=residue(b,a)` | Partial-fraction expansion | **[NICE]** |

### 9.3 Interpolation & numerical calculus

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `interp1` | `interp1(x,v,xq)`, `interp1(x,v,xq,method)`, `interp1(...,extrap)` | 1-D interpolation | Methods `'linear'`(default)`,'nearest','next','previous','spline','pchip','makima','cubic'`; `'extrap'` or scalar for outside. **[STD]** |
| `interp2` `interp3` `interpn` | `interp2(X,Y,V,Xq,Yq)` | 2-D/3-D/N-D grid interpolation | meshgrid vs ndgrid convention differs (interp2/3 use meshgrid; interpn uses ndgrid). **[NICE]** |
| `spline` | `spline(x,y,xq)`, `pp=spline(x,y)` | Cubic spline | Returns pp-form if no query points. **[NICE]** |
| `pchip` `makima` | `pchip(x,y,xq)` | Shape-preserving cubic interpolation | **[NICE]** |
| `ppval` `mkpp` `unmkpp` | `ppval(pp,xq)` | Evaluate/build/inspect piecewise polynomials | **[NICE]** |
| `griddata` `scatteredInterpolant` `griddedInterpolant` | | Scattered/gridded interpolation objects | **[NICE]** |
| `trapz` | `trapz(y)`, `trapz(x,y)`, `trapz(...,dim)` | Trapezoidal numerical integration | Unit spacing if no `x`. **[STD]** |
| `cumtrapz` | `cumtrapz(y)`, `cumtrapz(x,y)` | Cumulative trapezoidal integral | Same length as input. **[STD]** |
| `gradient` | `[fx,fy]=gradient(F)`, `gradient(F,h)`, `gradient(F,hx,hy)` | Numerical gradient (central differences) | One output per dimension; spacing scalars/vectors. **[STD]** |
| `diff` | (see §4.4) | Discrete differences | **[CORE]** |
| `del2` | `del2(U)` | Discrete Laplacian | **[NICE]** |
| `cumsum` `cumprod` | (see §4.4) | Cumulative operators | **[CORE]** |

---

## 10. Type & Introspection

| Function | Signature | Semantics | Notes |
|---|---|---|---|
| `class` | `class(obj)` | Class name as char | e.g. `'double'`,`'char'`,`'function_handle'`,`'cell'`,`'struct'`. **[CORE]** |
| `isa` | `isa(obj,classname)`, `isa(obj,'numeric'/'float'/'integer')` | Class/superclass test with abstract categories | Category tests (`'numeric'` etc.) are important. **[CORE]** |
| `isnumeric` `islogical` `ischar` `iscell` `isstruct` `isstring` `isreal` `isfloat` `isinteger` `isobject` | `isnumeric(A)` | Type predicates | Core set of `is*` checks. **[CORE]** |
| `isempty` `isscalar` `isvector` `ismatrix` | (see §4.1) | Shape predicates | **[CORE]** |
| `isnan` `isinf` `isfinite` | `isnan(A)` | Element-wise value predicates | **[CORE]** |
| `validateattributes` | `validateattributes(A,classes,attributes)` | Argument validation; errors if unmet | `classes` cellstr, `attributes` cellstr e.g. `{'positive','scalar','integer'}`. **[STD]** |
| `validatestring` | `validatestring(s,valid)` | Match to allowed option strings | **[STD]** |
| `inputname` | `inputname(k)` | Workspace variable name of k-th input arg | Returns `''` if arg was an expression. Function-context only. **[NICE]** |
| `nargin` `nargout` | `nargin`, `nargin(fn)`, `nargout` | Count of actual/declared in/out args | Negative from `narginchk`-style varargin declarations. **[CORE]** |
| `narginchk` `nargoutchk` | `narginchk(lo,hi)` | Validate arg counts | **[STD]** |
| `varargin` `varargout` | | Variable-length arg cells | Language feature, not a callable. **[CORE]** |
| `exist` | `exist(name)`, `exist(name,'var'/'file'/'builtin'/'dir'/'class')` | What a name resolves to | Returns code: 0 none, 1 var, 2 file, 5 built-in, 7 dir, 8 class, etc. **[STD]** |
| `isvarname` | `isvarname(s)` | Valid MATLAB identifier test | **[STD]** |
| `iskeyword` | `iskeyword(s)`, `iskeyword` | Reserved-word test / list | **[NICE]** |
| `who` `whos` | `who`, `s=whos` | List workspace variables / with details | `whos` struct has name/size/bytes/class. **[STD]** |
| `func2str` `str2func` | `str2func('sin')` | Function handle ↔ name | **[STD]** |
| `functions` | `functions(fh)` | Introspect a function handle (struct) | Diagnostic; documented as not for programmatic reliance. **[NICE]** |
| `func2str` | `func2str(fh)` | Handle → text | **[STD]** |
| `metaclass` `superclasses` `properties` `methods` `events` | `methods(obj)` | OOP introspection | Needed once classdef support exists. **[NICE]** |
| `is` (family: `isKey`, `ishandle`, ...) | | Misc predicates | Context-specific. **[NICE]** |

---

## Priority summary for language completeness

The categories that most determine whether a MATBOX runtime *feels like MATLAB*:

1. **Elementary math + arithmetic/relational/logical operators (§1, §6)** — the
   foundation; nearly every program uses these. All **[CORE]**.
2. **Array creation + manipulation + reductions (§3, §4)** — MATLAB *is* an array
   language; `zeros/ones/reshape/size/sort/find/sum/max/min` and friends are
   unavoidable.
3. **Strings (§7)** — large surface area; `sprintf`, `strcmp`, `num2str`,
   `str2double`, `regexp`, split/join are used constantly.
4. **Cell/struct + functional iteration (§8)** — `struct`, `fieldnames`, `cellfun`,
   `arrayfun` underpin idiomatic code.
5. **Linear algebra (§5)** — `\`, `inv`, `eig`, `svd`, `norm`, `det` are why people
   choose MATLAB; correctness here is high-value but can lean on a BLAS/LAPACK backend.
6. **Type/introspection (§10)** — needed for robust function authoring
   (`nargin`, `class`, `isa`, `validateattributes`).

Special functions (§2), advanced decompositions, page-wise ops, pattern objects, and
scattered interpolation are largely **[NICE]** and can be deferred.
