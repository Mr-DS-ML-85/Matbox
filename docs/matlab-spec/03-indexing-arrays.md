# MATLAB Indexing, Assignment, and Array Semantics

Clean-room reference for MATBOX. Behavior described in our own words from public
MathWorks documentation. Section-end **Sources** list the doc URLs consulted.

> Scope: indexing modes, assignment/growth, deletion, cell/comma-separated lists,
> implicit expansion, concatenation, multiple-output assignment, and the shape
> operators (`reshape`/`permute`/`squeeze`/`repmat`/`circshift`).

---

## 0. Foundational model: column-major storage

Every MATLAB array is stored as a single contiguous block in **column-major**
order: the columns are laid end-to-end. For a matrix `A` of size `m x n`, the
memory layout is `A(1,1), A(2,1), ..., A(m,1), A(1,2), A(2,2), ...`. This ordering
is the single most important fact for implementing indexing correctly, because it
defines linear indices, `reshape`, `(:)`, `find` order, and iteration order.

- Element `A(i,j)` has **linear index** `k = i + (j-1)*m` (1-based).
- Inverse (linear → subscripts) for an `m x n` array:
  `i = mod(k-1, m) + 1`, `j = floor((k-1)/m) + 1`.
- N-D generalization for size `[d1 d2 ... dN]` and subscripts `(s1,...,sN)`:
  `k = s1 + (s2-1)*d1 + (s3-1)*d1*d2 + ...` — i.e. earlier dimensions vary fastest.
  The stride of dimension `p` is `prod(d(1:p-1))`.
- **All MATLAB indices are 1-based.** Index `0`, negative, non-integer, `NaN`, or
  `Inf` subscripts are errors (except logical indices and the empty index).

Every array conceptually has infinitely many **trailing singleton dimensions**:
a `3x4` matrix is identical to `3x4x1x1...`. This underlies the "fewer subscripts
than dimensions" and "extra trailing subscripts of 1" rules below.

Sources:
- https://www.mathworks.com/help/matlab/math/array-indexing.html
- https://www.mathworks.com/company/technical-articles/matrix-indexing-in-matlab.html
- https://www.mathworks.com/help/matlab/math/detailed-rules-about-array-indexing.html

---

## 1. Indexing modes (reading)

### 1.1 Linear (single-subscript) indexing — `A(k)`
Uses one index regardless of the array's dimensionality; `k` walks the
column-major sequence. `A(6)` on a `3x3` returns the same element as `A(3,2)`.

**Shape-of-result rule (single subscript `id`):**
- In general, `A(id)` has the **same size and shape as the index array `id`**.
  `A([1 2;3 4])` returns a `2x2` array.
- **Vector exception:** if *both* `A` and `id` are vectors, the result takes the
  **orientation of `A`** (not of `id`). So if `A` is a row vector, `A(id)`
  is a row even when `id` is a column, and vice versa. (When `A` is a matrix,
  the shape follows `id`.)

### 1.2 Subscript indexing — `A(i,j)`, `A(i,j,k,...)`
- Two subscripts: `A(id1, id2)` is a rectangular submatrix with `numel(id1)`
  rows and `numel(id2)` columns. Result size is `numel(id1) x numel(id2)`.
- N subscripts: `A(id1,...,idn)` has size `numel(id1) x ... x numel(idn)`.
- Repeated indices are allowed and duplicate data: `A([1 1 1])` is legal.

**Fewer subscripts than dimensions (trailing-dimension folding):**
If you supply fewer subscripts than the array has dimensions, the **last supplied
subscript indexes the linearized product of all remaining dimensions.** A
`2x3x2` array addressed with two subscripts behaves as `2x6` (last two dims
folded, column-major). With one subscript it is full linear indexing over all 12
elements.

**More subscripts than dimensions:** allowed **only if every extra trailing
subscript equals `1`** (they address trailing singleton dims). `A(2,3,1,1)` is
fine for a matrix; `A(2,3,2)` errors.

### 1.3 Logical indexing — `A(mask)`
`A(A>0)` selects elements where the logical mask is `true`, in column-major
order. Rules:
- The mask is a logical array; a mask **shorter** than `A` is allowed (untested
  positions are treated as not selected); a mask **longer** than `A` errors, and
  any `true` beyond `numel(A)` is out of range.
- `A(mask)` for a single logical subscript returns a **column vector** in general
  (or preserves `A`'s orientation when `A` is a vector — same vector rule as 1.1).
- Logical masks combine with dimensions: `A(rowmask, :)` selects rows;
  `A(:, colmask)` selects columns.
- Numeric `0/1` is **not** a logical mask — `A([0 1 0])` is an index error
  (index 0). Only genuine `logical` type triggers mask semantics. This
  type-vs-value distinction must be preserved by the implementation.

### 1.4 Colon `:` and ranges
- `A(:, j)` — entire column `j`; `A(i, :)` — entire row `i`.
- `A(:)` — **linearize to a column vector** in column-major order. As an
  l-value, `A(:) = B` reshapes-in-place (see §2.4).
- Ranges use the colon operator `start:step:stop` (default step 1). `A(2:5)`,
  `A(1:2:end)`. An empty range (e.g. `5:1`) yields an empty selection.

### 1.5 The `end` keyword
Inside an index expression, `end` evaluates to the **size of `A` along the
dimension in whose subscript position it appears**, given the current number of
subscripts. Key rules:

- Position-dependent: in `A(i, j)`, `end` in slot 1 = number of rows, in slot 2 =
  number of columns. `A(end, end)` is the last element of a matrix.
- Under folding: with `k` subscripts on an N-D array (`k < N`), `end` in the last
  supplied slot equals the **product of the remaining dimensions** (consistent
  with §1.2). `A(end)` with a single subscript equals `numel(A)`.
- Arithmetic allowed: `A(end-1)`, `A(end-1:end)`, `A(2:end, :)`,
  `A(end/2)`, `A(end:-1:1)` (reverse).
- `end` is only meaningful inside `()`/`{}` indexing (and its nested index
  expressions). Each `end` binds to the array and dimension of the innermost
  indexing operation it belongs to.
- Implementation note: `end` must be resolved **per subscript slot** before
  evaluating that subscript, and it depends on the arity of the indexing call.

Sources:
- https://www.mathworks.com/help/matlab/math/detailed-rules-about-array-indexing.html
- https://www.mathworks.com/help/matlab/math/array-indexing.html

---

## 2. Assignment and automatic array growth

### 2.1 In-range assignment — `A(idx) = rhs`
- Scalar RHS is **scalar-expanded** to fill all selected positions:
  `A(1:3) = 0` sets three elements to 0.
- Non-scalar RHS must have **exactly `numel(selected)` elements**; shape need not
  match (it is filled column-major), but the count must match or it errors
  (`Unable to perform assignment because the left and right sides have a
  different number of elements`).
- `A(mask) = rhs`: assigns to masked positions; RHS is scalar or has
  `nnz(mask)` elements.

### 2.2 Reading out of range vs. assigning out of range
- **Read** past the end → error (`Index exceeds the number of array elements`
  / `Index in position K exceeds array bounds`).
- **Assign** past the end → the array **auto-grows**; newly created elements are
  **zero-filled** (for numeric), `false` for logical, `''`/space-like for char,
  `[]` (empty) cells for cell arrays, and the type's default for other classes.

### 2.3 Growth by linear assignment
`A(k) = v` where `k > numel(A)`:
- If `A` is **empty or a vector (or scalar/1x1)**, it grows into a vector,
  preserving orientation (a row stays a row, a column stays a column; empty and
  scalars grow to a **row** vector by default). Intermediate positions are
  zero-filled. Example: `A = [1 2 3]; A(6) = 9;` → `[1 2 3 0 0 9]`.
- If `A` is a **non-vector matrix**, linear-index growth past `numel(A)` is an
  **error** — you cannot linearly grow a 2-D+ matrix (its shape is ambiguous).
  `Unable to resize... ` / `In an assignment A(I) = B, the number of elements
  in B and I must be the same` variants apply.

### 2.4 `A(:) = B` (colon assignment)
Assigns into the existing array **without changing its size**. `B` must have
`numel(A)` elements; values fill column-major. This never grows or reshapes the
target's dimensions — it is a shape-preserving bulk write.

### 2.5 Growth by subscript assignment (2-D and N-D)
`A(i,j) = v` where `i` or `j` exceeds current size grows **each dimension
independently** to `max(oldsize, index)`, zero-filling the rest.
- `A = []; A(2,3) = 1;` → a `2x3` matrix, zeros everywhere except `A(2,3)=1`.
- Growth can enlarge multiple dimensions at once; all new cells are zero-filled.
- N-D: `A(i,j,k)=v` extends to `max` along each of the three dims and creates the
  needed trailing dimension if `k>1`.
- Assigning with a subscript arity **greater than current ndims** is allowed and
  creates new trailing dimensions (subject to the trailing-singleton rules).

### 2.6 Type of the growing array
The array keeps its class. Assigning a value of a **different class** does not
change `A`'s class; the RHS is converted to `A`'s class (e.g. writing `3.7` into
an `int8` array stores `4` with saturation; writing into a `logical` array
requires `0/1`). Assigning into a previously-undefined variable creates it with
the RHS class.

Sources:
- https://www.mathworks.com/help/matlab/math/resizing-and-reshaping-matrices.html
- https://www.mathworks.com/help/matlab/math/creating-and-concatenating-matrices.html
- https://www.mathworks.com/help/matlab/math/detailed-rules-about-array-indexing.html

---

## 3. Element deletion — assigning `[]`

Deletion removes elements by assigning the empty matrix `[]` to an index
expression. Rules:

- **Linear/vector deletion:** `A(2) = []` removes the 2nd element and the array
  **collapses** (remaining elements shift; a vector stays a vector). `A([2 4]) =
  []` removes multiple. `A(A>5) = []` deletes by mask.
- **Row/column deletion:** `A(2,:) = []` deletes row 2; `A(:,3) = []` deletes
  column 3. The remaining array closes the gap and stays rectangular.
- **The null-assignment restriction (critical):** you may delete along **at most
  one dimension**; every other subscript in the deletion must be the full colon
  `:`. `A(2,3) = []` is an **error** (`A null assignment can only have one
  non-colon index`) because deleting a single interior element would leave a
  non-rectangular array.
- Deleting all of a dimension yields an empty array with a preserved sensible
  shape (e.g. deleting every column of an `m x n` gives `m x 0`).
- The RHS must be exactly `[]` (the `0x0` double empty), used as a null
  assignment; other empties in this position follow the same null-assignment
  path.

Sources:
- https://www.mathworks.com/help/matlab/math/deleting-array-elements.html
- https://www.mathworks.com/help/matlab/matlab_prog/add-cells-to-a-cell-array.html

---

## 4. Cell arrays and comma-separated lists (CSLs)

### 4.1 Two indexing operators on cells
- **`C(...)` (parentheses)** returns a **sub-cell-array** (a cell containing the
  selected cells). `C(1:2)` is a `1x2` cell. Assignment `C(i) = {val}` replaces
  cells; RHS must be a cell.
- **`C{...}` (braces)** returns the **contents** of the selected cells. A single
  `C{i}` returns the stored value. **Multiple** `C{:}` / `C{1:3}` produce a
  **comma-separated list** — several distinct values, not an array.
- Assignment `C{i} = val` stores `val` into that cell (any type). `[C{:}] =
  deal(...)` writes to multiple cells (see §7).

### 4.2 Comma-separated lists
`C{:}` and `S.field` (a field read across a **struct array**) each expand to a
CSL — literally the sequence `v1, v2, v3, ...` as if typed out separated by
commas. A CSL is **not** a first-class value; it only has meaning in these
contexts:

1. **Function arguments:** `f(C{:})` passes each element as a separate positional
   argument. `plot(x, y, C{:})` splices option/value pairs. Empty cell `C{:}`
   with `C = {}` passes **zero** arguments.
2. **Concatenation `[ ]` / `{ }`:** `[C{:}]` concatenates the elements into one
   array (must be size/type compatible per §6). `{S.field}` gathers a struct
   array's field values into a new cell array. `[S.field]` concatenates them.
3. **Multiple-output assignment:** `[a,b,c] = C{1:3}` (or `[a,b] = S.field`)
   distributes the list left-to-right. If there are **more** list items than
   targets, the extras are silently ignored **only** in the `[a,b]=C{1:n}` cell
   case where fewer targets are given; a plain over/under count against a
   function call errors. Fewer list items than targets errors.
4. **Struct-array field read:** `S.x` where `S` is `1xN` yields `N` values as a
   CSL, in array order.

### 4.3 Restrictions
- A brace index that yields **more than one value cannot be followed by further
  indexing**: `C{1,:}(2)` is an error (`Intermediate brace indexing produced a
  comma-separated list ... must produce a single value`). Index the single cell
  first: `C{1,2}(2)`.
- A CSL cannot be stored in a variable directly; `x = C{:}` assigns only the
  **first** value and warns/errors depending on context (in an assignment with a
  single LHS, only the first element is taken).

Sources:
- https://www.mathworks.com/help/matlab/matlab_prog/comma-separated-lists.html
- https://www.mathworks.com/help/matlab/matlab_prog/access-data-in-cell-array.html
- https://www.mathworks.com/help/matlab/matlab_prog/access-data-in-a-structure-array.html

---

## 5. Implicit expansion (broadcasting), R2016b+

### 5.1 Compatibility rule
Two arrays have **compatible sizes** if, **for every dimension, the sizes are
either equal or one of them is `1`.** During an element-wise operation MATLAB
implicitly expands (virtually replicates) each size-1 dimension to match the
other operand. Prior to R2016b only scalar expansion was allowed; the general
rule now applies to all supported operators.

Compare dimension-by-dimension after **padding the shorter shape with trailing
`1`s** (trailing singleton rule). Result size in each dimension is
`max(sizeA(d), sizeB(d))`.

### 5.2 Examples
- scalar ∘ any array → array (classic scalar expansion).
- `4x2` ∘ `4x1` (column) → `4x2`.
- `1x3` (row) ∘ `2x1` (column) → `2x3` (outer-product-shaped grid).
- `3x4` ∘ `3x4x2` → `3x4x2`.
- `4x3` ∘ `1x3x3` → `4x3x3`.
- **Incompatible:** `3x2` ∘ `4x2` (dim1: 3 vs 4, neither is 1) → error
  `Arrays have incompatible sizes for this operation`. `1x3` ∘ `1x4` → error.

### 5.3 Empty arrays
Same rule with `0` participating: a `0` dimension is compatible only with `0` or
`1`, and the result keeps the `0` (empty propagates).

### 5.4 Operators/functions that implicitly expand
- Element-wise arithmetic: `+ - .* ./ .\ .^` (and unary-free binary forms).
- Relational: `< <= > >= == ~=`.
- Logical: `& | xor`.
- Bit-wise: `bitand bitor bitxor`.
- Two-input elementary math: `max min mod rem hypot atan2 atan2d`.
- **Matrix** `* / \ ^` do **not** broadcast — they use linear-algebra size rules.

### 5.5 Implementation gotchas
- Broadcasting also applies **across `=`** only in the sense that RHS may be
  scalar-expanded on assignment; general broadcasting is for the operators above,
  computed **before** assignment.
- A `1xN` op `Nx1` silently produces `NxN` — a frequent accidental-blowup bug you
  should reproduce faithfully (do **not** error). Historically (pre-R2016b) this
  was a dimension-mismatch error; MATBOX targets modern semantics.

Sources:
- https://www.mathworks.com/help/matlab/matlab_prog/compatible-array-sizes-for-basic-operations.html
- https://blogs.mathworks.com/loren/2016/10/24/matlab-arithmetic-expands-in-r2016b/
- https://www.mathworks.com/help/matlab/ref/bsxfun.html

---

## 6. Concatenation

### 6.1 Bracket syntax and `cat`
- `[A, B]` / `[A B]` — horizontal concat (== `horzcat`, == `cat(2,...)`);
  operands must have equal row counts (and equal sizes in all dims except 2).
- `[A; B]` — vertical concat (== `vertcat`, == `cat(1,...)`); equal column counts
  (equal sizes in all dims except 1).
- `cat(dim, A1, ..., An)` — concatenate along `dim`; all operands must match in
  **every dimension except `dim`**. `cat(3,A,B)` stacks matrices into pages.
- A bracketed expression combines both: rows built by `,`, stacked by `;`, and
  each row must produce equal-height blocks and each column equal-width blocks.

### 6.2 Size compatibility
Along the concat dimension sizes add; along every other dimension they must be
**equal** (trailing singletons apply). Mismatch → `Dimensions of arrays being
concatenated are not consistent`.

### 6.3 Empty `[]` behavior
The `0x0` empty `[]` is the **identity** for concatenation: `[A, []]`, `[[]; A]`,
`[[] A []]` all yield `A`. This is why `[]` is used both to build empties and to
"drop" pieces. A non-`0x0` empty (e.g. `0x3`) participates by its actual
dimensions and can conflict (`[A; zeros(0,2)]` needs column match). Concatenating
only empties yields an empty whose shape follows the non-conflicting dimensions.

### 6.4 Mixing types (class of the result)
When operands differ in class, MATLAB applies a **dominance hierarchy** and
converts all operands to the dominant class:
- **char** dominates **double**: `['A' 66]` → `'AB'` (double 66 → char 'B').
  `[65 'B']` likewise becomes char. (char + char is plain char concat.)
- **integer** classes dominate `double`/`single`/`char`/`logical`: `[int8(1)
  2.9]` → `int8` `[1 3]` (RHS converted with rounding + **saturation** to the
  integer range). Mixing **two different integer classes** (e.g. `int8` + `int16`)
  is an **error**.
- **single** dominates **double**: result is `single`.
- **logical** and **char** are **subordinate** to numeric: concatenating with any
  numeric yields the numeric/char class (logical → its numeric partner). `[true
  2]` → `double [1 2]`.
- **Cell**: `[C1 C2]` concatenates cell arrays (cells combined); to wrap values
  into a cell use `{a, b}`.
- **struct**: concatenation allowed when structs have the **same set of field
  names** (order-independent); result is a struct array.
- Otherwise, a common `class`/converter must exist or it errors.

Practical rule for the implementer: compute a **result class** by folding the
dominance rule over all operands, verify it is representable, then convert each
operand to that class (with integer rounding/saturation) before laying out data.

Sources:
- https://www.mathworks.com/help/matlab/math/creating-and-concatenating-matrices.html
- https://www.mathworks.com/help/matlab/ref/cat.html
- https://www.mathworks.com/help/matlab/matlab_prog/combining-unlike-integer-types.html
- https://www.mathworks.com/help/matlab/matlab_prog/valid-combinations-of-unlike-classes.html

---

## 7. Multiple outputs, `deal`, `~`, and `nargout`

### 7.1 `[a, b, ...] = expr`
- Multiple LHS targets capture successive outputs of a function or a CSL.
  `[m, n] = size(A)`, `[val, idx] = max(x)`.
- The number of requested outputs sets `nargout` inside the callee.
- Requesting **more** outputs than the function can supply is an error; requesting
  **fewer** is fine (uncomputed outputs simply are not produced).

### 7.2 Ignoring outputs with `~`
- `~` in an output slot discards that output: `[~, idx] = max(x)`,
  `[~, ~, v] = find(x)`. Consecutive tildes are comma-separated.
- `~` still **counts toward `nargout`** (the position is requested, just not
  bound to a name), so a function can compute it. Whether it was *specifically*
  suppressed is not directly observable through standard `nargout`.

### 7.3 `deal`
- `[a, b, c] = deal(x)` copies the single input `x` to **all** outputs.
- `[a, b, c] = deal(x, y, z)` is a straight one-to-one distribution (requires
  `nargin == nargout`).
- Common idiom: `[C{:}] = deal(v)` fills every cell of `C`; `[S.field] =
  deal(v)` sets a field across a whole struct array.

### 7.4 `nargout`-dependent function behavior
Many builtins change results based on how many outputs are requested — the
implementation must branch on `nargout`:
- `size`: `s = size(A)` → full size row vector; `[m,n] = size(A)` → 2 values, and
  crucially with `k < ndims` outputs the **last output collapses the product of
  all remaining dimensions** (mirrors index folding, §1.2). `[m,n,p]=size(A)`
  pads with trailing `1`s if `A` has fewer dims.
- `max`/`min`: 1 output = values; 2 outputs = `[values, indices]` (index of first
  occurrence of the max along the operating dim).
- `sort`: 1 output = sorted data; 2 outputs = `[sorted, permIndices]` such that
  `sorted = A(permIndices)` along the sort dim.
- `find`: 1 output = linear indices; 2 outputs = `[row, col]` subscripts; 3
  outputs = `[row, col, values]`.
- `unique`: 1 output = unique values; up to 3 = `[C, ia, ic]` with `C = A(ia)`
  and `A = C(ic)` (index conventions per the current sort/stable option).
- `ismember`, `histc`, `eig`, `svd`, `deal`, etc. follow the same
  nargout-branching pattern.

Sources:
- https://www.mathworks.com/help/matlab/matlab_prog/ignore-function-outputs.html
- https://www.mathworks.com/help/matlab/ref/nargout.html
- https://www.mathworks.com/help/matlab/ref/deal.html
- https://www.mathworks.com/help/matlab/ref/size.html

---

## 8. Reshape / permute / squeeze / repmat / circshift

All of these respect column-major order.

### 8.1 `reshape(A, sz)` / `reshape(A, m, n, ...)`
- Rearranges elements into the new shape **without moving data in memory** — it
  reinterprets the same column-major stream. `numel` must be preserved.
- **Exactly one** dimension may be `[]`, meaning "infer from the rest":
  `reshape(A, 2, [])`. If the product of the specified dims does not divide
  `numel(A)`, it errors.
- Fast/O(1) conceptually (header change only); implementation may keep data
  buffer and only rewrite the size descriptor.

### 8.2 `permute(A, order)` / `ipermute`
- Reorders dimensions: result dim `i` is `A`'s dim `order(i)`. `permute(A,[2 1])`
  is the (non-conjugate) transpose for a matrix. `order` must be a permutation of
  `1:max(ndims, numel(order))`.
- Unlike `reshape`, this **physically rearranges** data (strides change).
- `ipermute(permute(A,p),p) == A`.

### 8.3 `squeeze(A)`
- Removes **singleton dimensions** (size-1), except it never reduces below 2-D:
  a result is at least a matrix. `squeeze` of a `1x1xN` → `Nx1` column;
  `squeeze` of a `2x1x3` → `2x3`. Row/column vectors are unchanged (already 2-D).

### 8.4 `repmat(A, r, c)` / `repmat(A, [r c ...])`
- Tiles copies of `A` in a grid: output size is `size(A) .* reps`. `repmat(A,2,3)`
  makes a `2x3` block grid of `A`. `repmat(v, n, 1)` stacks `n` copies of a row.
- Works for any class including cell/char/struct; scalar `repmat(A,n)` gives an
  `n x n` tiling.

### 8.5 `circshift(A, K [, dim])`
- Circularly shifts elements; values pushed off one end wrap to the other. No
  data is lost, size is unchanged.
- Scalar `K` with no `dim`: shifts along the **first non-singleton dimension**.
- `K` as a vector: `K(d)` is the shift amount along dimension `d`.
- With `dim`: shift only along that dimension. Positive `K` shifts toward higher
  indices (down a column / right along a row).

Sources:
- https://www.mathworks.com/help/matlab/ref/reshape.html
- https://www.mathworks.com/help/matlab/ref/permute.html
- https://www.mathworks.com/help/matlab/ref/squeeze.html
- https://www.mathworks.com/help/matlab/ref/repmat.html
- https://www.mathworks.com/help/matlab/ref/circshift.html
- https://www.mathworks.com/help/matlab/math/reshaping-and-rearranging-arrays.html

---

## Implementer quick-reference (invariants)

1. Store column-major; derive linear↔subscript with strides `prod(d(1:p-1))`.
2. 1-based indexing everywhere; only `logical` type (not `0/1` numeric) triggers
   mask semantics; empty index selects nothing.
3. `end` resolves per subscript slot and depends on the indexing arity (folding).
4. Single-subscript result shape = index shape, except both-vectors → follow
   `A`'s orientation.
5. Reads out of range error; assigns out of range grow + zero-fill. Linear growth
   only for vectors/empties, never for 2-D+ matrices.
6. Deletion (`=[]`) permits non-colon indexing along at most one dimension.
7. `{}`/`.field`-across-struct-array produce comma-separated lists; splice into
   args, `[]`/`{}`, and `[a,b]=...`. A multi-value brace index cannot be chained.
8. Implicit expansion: per-dim `equal or one is 1`; only the listed element-wise
   operators/functions; matrix `* / \ ^` do not broadcast.
9. Concatenation result class via dominance (char>double, integer>float with
   round+saturate, single>double, two distinct int classes error); `0x0 []` is
   the concat identity.
10. Branch builtins (`size`,`max`,`sort`,`find`,`unique`) on `nargout`; `~`
    still counts as a requested output.
