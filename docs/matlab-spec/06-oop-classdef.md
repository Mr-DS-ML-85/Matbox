# MATLAB OOP: `classdef` and Namespaces

> **Clean-room reference for MATBOX.** All behavior below is described in the
> implementer's own words from public MathWorks documentation
> (`mathworks.com/help`). No MathWorks source code is reproduced. Doc URLs are
> cited per section.

This document specifies the MATLAB object-oriented programming model:
`classdef` file structure, value vs. handle semantics, properties, methods,
inheritance, operator overloading, events, enumerations, and namespaces.

---

## 1. `classdef` File Structure

A class is defined in a single `.m` file (or an `@ClassName` folder) whose name
matches the class. The definition is bracketed by `classdef ... end` and
contains attribute-scoped **blocks**.

```matlab
classdef (Attr1 = val1, Attr2) ClassName < SuperA & SuperB
    properties (PropertyAttributes)
        PropName SizeClass {validators} = defaultValue
    end
    methods (MethodAttributes)
        function obj = ClassName(args)   % constructor
            ...
        end
        function out = someMethod(obj, args)
            ...
        end
    end
    events (EventAttributes)
        EventName
    end
    enumeration
        Member1, Member2
    end
end
```

### Block types
| Block | Purpose |
|-------|---------|
| `properties` | Declare property names, attributes, validation, defaults. |
| `methods` | Define functions (including constructor, static methods, get/set methods, operator overloads). |
| `events` | Declare named events (handle classes only). |
| `enumeration` | Define the fixed set of members of an enumeration class. |

A class may contain **multiple blocks of each kind**, each with a different
attribute set (e.g. one `properties (Constant)` block and one
`properties (Access = private)` block).

### Attribute syntax
- Attributes are set in parentheses after the block/`classdef` keyword:
  `properties (SetAccess = private)`.
- Logical attributes may use shorthand: `methods (Static)` ≡
  `methods (Static = true)`; a `~` prefix means `false`.
- All attributes have documented default values; specify only deviations.

### Class attributes
| Attribute | Default | Meaning |
|-----------|---------|---------|
| `Abstract` | `false` | Class cannot be instantiated; serves as a base/interface. |
| `Sealed` | `false` | Class cannot be subclassed. |
| `ConstructOnLoad` | `false` | Constructor is called when loading an object from a MAT-file (constructor must accept zero args). |
| `HandleCompatible` | `false` | A value class marked so can be a superclass of a handle class (used for mixins). |
| `Hidden` | `false` | Class omitted from `superclasses`/`help` output. |
| `AllowedSubclasses` | (all) | Restricts which classes may subclass this one (`?Name` metadata list); empty list ≈ `Sealed`. |
| `InferiorClasses` | (none) | Establishes dispatch precedence relative to listed classes. |

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/class-components.html
- https://www.mathworks.com/help/matlab/matlab_oop/class-attributes.html

---

## 2. Value Classes vs. Handle Classes (the central distinction)

This is the most important semantic decision for an implementer.

### Value classes (default)
`classdef MyValue ... end` (no superclass, or subclassing only other value
classes). **Copy semantics:**
- Assigning an object to a new variable creates an **independent copy**.
- Passing to a function passes a copy; the function's changes are **not** visible
  in the caller unless the modified object is **returned and reassigned**.
- Two value objects are equal (`==`, `isequal`) when they are of the same class
  and have equal property values.
- No `delete`; the object simply goes away when its variable is cleared or goes
  out of scope.

Mutation pattern for value classes (must return `obj`):
```matlab
function obj = setName(obj, n)
    obj.Name = n;      % modifies the local copy
end
% caller MUST reassign:
a = a.setName("x");    %  a = setName(a,"x")
```

### Handle classes
`classdef MyHandle < handle ... end` (subclass of the built-in `handle`).
**Reference semantics:**
- A variable holds a **handle (reference)** to the object. Assigning it to
  another variable copies only the reference; both refer to the **same** object.
- Passing to a function passes the handle; the function can mutate the object
  **in place** and the caller sees the change — **no return needed**.
- `delete(h)` destroys the object; all handles to it become invalid
  (`isvalid(h)` → false).
- `==` tests **identity** (same underlying object). `isequal` additionally
  returns true for distinct objects that hold equal property values.
- Only handle classes may define **events**, **listeners**, and observable
  properties, and only they support `matlab.mixin.Copyable`.

### Implementer's model
| Aspect | Value class | Handle class |
|--------|-------------|--------------|
| Assignment `b = a` | deep-ish copy of state | copy of reference |
| Pass to function | by copy | by reference |
| Method mutation | must return `obj` | mutates in place |
| `==` | state equality | identity |
| Lifetime | scope-bound | explicit `delete` |

Source:
- https://www.mathworks.com/help/matlab/matlab_oop/comparing-handle-and-value-classes.html

---

## 3. Properties

### Declaration and validation syntax
```matlab
properties
    PropName (dim1,dim2) ClassName {validator1, validator2} = defaultValue
end
```
All four positions after the name are optional. Order and semantics:
1. **Size** `(dim1,dim2,...)` — each dimension is a positive integer or `:`
   (any length). The assigned value must match or be compatible (scalar
   expansion is applied).
2. **Class** `ClassName` — restricts to a single class; assigned values are
   converted following MATLAB conversion rules (e.g. `char`→`string`).
3. **Validators** `{f1, f2, ...}` — functions such as `mustBePositive`,
   `mustBeNonnegative`, `mustBeMember(x,set)`, `mustBeText`. They error on
   failure and return nothing.
4. **Default** `= value` — must itself satisfy the validation.

**Validation order on assignment:** class conversion → size (incl. scalar
expansion) → validators (left→right, first error stops) → `set.Prop` method (if
any). Default values are validated once at class load.

### Property attributes
| Attribute | Default | Meaning |
|-----------|---------|---------|
| `Access` | `public` | Sets both Get and Set access at once. |
| `GetAccess` | `public` | Read visibility: `public`/`protected`/`private`/class list. |
| `SetAccess` | `public` | Write visibility: as above, plus `immutable` (set only in constructor). |
| `Constant` | `false` | Same value for all instances; no per-object storage; set in declaration. |
| `Dependent` | `false` | Not stored; value computed by a `get.Prop` method on access. |
| `Transient` | `false` | Excluded from `save`/serialization. |
| `Abstract` | `false` | Must be implemented by concrete subclass (no default, no storage). |
| `Hidden` | `false` | Omitted from `properties`/`get`/`set` listings. |
| `AbortSet` | `false` | (handle) Skip the set if the new value equals the current value. |
| `SetObservable`/`GetObservable` | `false` | (handle) Enables `PostSet`/`PreSet`/`PostGet`/`PreGet` property-event listeners. |
| `NonCopyable` | `false` | (handle) Property not duplicated by `copy`. |

`Access`/`GetAccess`/`SetAccess` accept: `public`, `protected` (this class +
subclasses), `private` (this class only), `immutable` (SetAccess only), or a
metadata class list `{?ClassA, ?ClassB}`.

### Get / set access methods
```matlab
methods
    function v = get.Prop(obj)          % called on every read/display of Prop
        v = ...;
    end
    function obj = set.Prop(obj, v)     % value class: MUST return obj
        obj.Prop = v;
    end
    function set.Prop(obj, v)           % handle class: no return
        obj.Prop = v;
    end
end
```
Rules:
- MATLAB calls `get.Prop`/`set.Prop` automatically on access/assignment,
  including within the constructor — **but not** during default initialization
  or when copying a value object's properties.
- Access methods are **not** recursive: inside `get.Prop`, referencing `obj.Prop`
  accesses the stored value directly (no re-entry).
- **Dependent** properties store nothing and require a `get.Prop` method; they
  need a `set.Prop` only if assignable.

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/how-to-use-properties.html
- https://www.mathworks.com/help/matlab/matlab_oop/validate-property-values.html
- https://www.mathworks.com/help/matlab/matlab_oop/property-set-methods.html
- https://www.mathworks.com/help/matlab/matlab_oop/property-attributes.html

---

## 4. Methods

### Ordinary methods and the explicit object argument
Non-static methods receive the object as an **explicit first argument** (by
convention `obj`). MATLAB has **no** implicit `this`/`self`.
```matlab
methods
    function out = area(obj)
        out = obj.Width * obj.Height;
    end
end
```

### Dispatch and calling syntax
These two calls are equivalent:
```matlab
r.area()          % dot syntax
area(r)           % function syntax
r.setW(3)   <=>   setW(r, 3)
```
For dot syntax `obj.method(args)`, MATLAB dispatches on the **class of `obj`**.
For function syntax `method(obj, args)`, MATLAB dispatches on the class of the
argument(s) — user classes outrank built-in types (see §6 precedence).

### Constructors
- A constructor is an ordinary (non-static) method whose **name equals the class
  name** and which **returns the object** (typically `function obj = ClassName(...)`).
- It should initialize `obj`. If it subclasses, it may call superclass
  constructors (see §5).
- With `ConstructOnLoad`, it must be callable with no arguments.

### Static methods
```matlab
methods (Static)
    function y = helper(x)   % no object argument
        y = ...;
    end
end
```
Called as `ClassName.helper(x)`. Do not take an object; used for
factory/utility functions. Constructors **cannot** be static.

### Method attributes
| Attribute | Default | Meaning |
|-----------|---------|---------|
| `Access` | `public` | `public`/`protected`/`private`/`{?ClassList}`. |
| `Static` | `false` | No object argument; call via class name. |
| `Abstract` | `false` | Signature only (no `function...end` body); subclass must implement. |
| `Sealed` | `false` | Cannot be overridden in a subclass. |
| `Hidden` | `false` | Omitted from `methods`/`ismethod`. |

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/methods-in-class-design.html
- https://www.mathworks.com/help/matlab/matlab_oop/class-constructor-methods.html
- https://www.mathworks.com/help/matlab/matlab_oop/static-methods.html
- https://www.mathworks.com/help/matlab/matlab_oop/method-attributes.html

---

## 5. Inheritance

### Subclassing
```matlab
classdef Sub < Base            % single inheritance
classdef Sub < BaseA & BaseB   % multiple inheritance
```
A subclass inherits properties and methods of its superclasses and may add,
override, or (for `Abstract` members) implement them.

### Calling superclass constructors
Use the `obj@SuperclassName(args)` syntax inside the subclass constructor:
```matlab
classdef Stocks < finance.Assets & Taxable
    methods
        function s = Stocks(assetArgs, taxArgs)
            s@finance.Assets(assetArgs);   % fully qualify namespaced supers
            s@Taxable(taxArgs);
        end
    end
end
```
Rules:
- Call **only direct** superclasses (each superclass calls its own supers).
- If not called explicitly, MATLAB calls each superclass constructor with **no
  arguments**, left-to-right in the order listed after `<`.
- A superclass constructor call **cannot be conditionalized** (not inside
  `if`/`for`); you may compute argument values conditionally beforehand.

### Overriding and abstract members
- To **override**, define a method/property of the same name in the subclass
  (unless the superclass marked it `Sealed`).
- Access a superclass method implementation from an override via
  `result = method@Superclass(obj, args)`.
- **Abstract** methods/properties (class or member attribute) declare an
  interface; a subclass becomes **concrete** only when all inherited abstract
  members are implemented. Concrete overrides of abstract properties must keep
  the same `GetAccess`/`SetAccess`.

### Handle mixins / multiple inheritance
Multiple inheritance is commonly used to add **handle mixins** (e.g.
`matlab.mixin.Copyable`, `matlab.mixin.SetGet`, `dynamicprops`) alongside a main
superclass: `classdef C < handle & matlab.mixin.Copyable`.

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/subclass-constructors.html
- https://www.mathworks.com/help/matlab/matlab_oop/abstract-classes-and-interfaces.html

---

## 6. Operator Overloading

Each operator maps to a **method name**; defining that method in a class
overloads the operator for instances of the class. User-defined classes have
**higher dispatch precedence** than built-in types, and overloads keep the
operator's original precedence/associativity.

| Expression | Method | Expression | Method |
|-----------|--------|-----------|--------|
| `a + b` | `plus` | `a == b` | `eq` |
| `a - b` | `minus` | `a ~= b` | `ne` |
| `-a` | `uminus` | `a < b` | `lt` |
| `+a` | `uplus` | `a > b` | `gt` |
| `a .* b` | `times` | `a <= b` | `le` |
| `a * b` | `mtimes` | `a >= b` | `ge` |
| `a ./ b` | `rdivide` | `a & b` | `and` |
| `a .\ b` | `ldivide` | `a \| b` | `or` |
| `a / b` | `mrdivide` | `~a` | `not` |
| `a \ b` | `mldivide` | `a : b` | `colon` |
| `a .^ b` | `power` | `a'` | `ctranspose` |
| `a ^ b` | `mpower` | `a.'` | `transpose` |
| `[a b]` | `horzcat` | `[a; b]` | `vertcat` |
| `a(...)` | `subsref` | `a(...) = b` | `subsasgn` |
| display of `a` | `disp` / `display` | equality set | `eq`, `isequal` |

Common non-operator overloads: `disp`, `display`, `numel`, `size`, `end`,
`cat`, `char`, `double`.

### Indexed reference/assignment (`subsref` / `subsasgn`) — dispatch model
These are the deepest part of the model. MATLAB translates **all** indexing on
an object into calls:
```matlab
B = subsref(A, S)          % for reads:  A(i), A{i}, A.name
A = subsasgn(A, S, val)    % for writes: A(i)=val, A{i}=val, A.name=val
```
`S` is a **struct array**; each element has:
- `S(k).type` — one of `'()'`, `'{}'`, `'.'`
- `S(k).subs` — a cell array of subscripts for `()`/`{}`, or a char/string field
  name for `'.'`.

Translation examples:
| Source | `S` |
|--------|-----|
| `A(1:2,:)` | `type='()'`, `subs={1:2, ':'}` |
| `A{3}` | `type='{}'`, `subs={3}` |
| `A.name` | `type='.'`, `subs='name'` |

**Chained indexing** produces a multi-element `S`. `A{1}.field(3:5)` yields:
```
S(1): type='{}', subs={1}
S(2): type='.',  subs='field'
S(3): type='()', subs={[3 4 5]}
```
Helpers: `substruct('()',{...})` builds `S`; a class may also define
`numArgumentsFromSubscript` (and legacy `numel`) so MATLAB knows how many
outputs a subscripted expression yields (important for comma-separated lists /
`{}` results).

> **Modern alternative (R2021b+):** rather than overloading `subsref`/`subsasgn`
> directly, MATLAB now recommends the modular mixins
> `matlab.mixin.indexing.RedefinesParen` (`()`),
> `RedefinesDot` (`.`), and `RedefinesBrace` (`{}`). A class cannot both inherit
> those and overload `subsref`/`subsasgn`. For MATBOX, implement the classic
> `subsref`/`subsasgn` model first (broadest compatibility); the mixins are an
> advanced convenience layer over the same dispatch.

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/implementing-operators-for-your-class.html
- https://www.mathworks.com/help/matlab/ref/subsref.html
- https://www.mathworks.com/help/matlab/matlab_oop/indexed-reference-and-assignment.html

---

## 7. Events and Listeners (handle classes only)

Events model the observer pattern and are available **only in handle classes**
(a value copy could not receive callbacks on the originating object).

```matlab
classdef Source < handle
    events
        DataChanged           % declared event name
    end
    methods
        function update(obj, v)
            obj.Value = v;
            notify(obj, 'DataChanged');          % broadcast to listeners
        end
    end
end

% Attach a listener:
src = Source;
lh = addlistener(src, 'DataChanged', @(s,e) disp('changed!'));
```

Mechanics:
- `notify(source, 'EventName')` or `notify(source, 'EventName', eventData)`
  broadcasts to every listener registered for that event on that object.
- Listener callbacks have signature `@(source, eventData)`. By default
  `eventData` is an `event.EventData` object with `EventName` and `Source`
  properties; subclass `event.EventData` to carry extra data.
- **`addlistener(obj, event, cb)`** ties the listener's lifetime to the source
  object (destroyed with it). **`listener(obj, event, cb)`** creates a listener
  the caller owns/manages (no coupling). Both return an `event.listener` handle;
  keeping it alive keeps the listener active.
- **Property events**: with `SetObservable`/`GetObservable` properties, listen
  for the predefined `PreSet`/`PostSet`/`PreGet`/`PostGet` events.

Event attributes: `NotifyAccess` (who may `notify` / trigger — default `public`)
and `ListenAccess` (who may register listeners — default `public`); values
`public`/`protected`/`private`/`{?ClassList}`.

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/events-and-listeners-concepts.html
- https://www.mathworks.com/help/matlab/matlab_oop/event-attributes.html
- https://www.mathworks.com/help/matlab/ref/handle.addlistener.html
- https://www.mathworks.com/help/matlab/ref/handle.notify.html

---

## 8. Enumeration Classes

An `enumeration` block defines a fixed set of named members.
```matlab
classdef Weekday
    enumeration
        Monday, Tuesday, Wednesday
    end
end
% reference:
d = Weekday.Tuesday;
```
Members are referenced with dot notation `ClassName.Member` and are instances of
the class. Enumeration value objects are **immutable**.

**Subclassing a built-in type** gives each member an underlying numeric/string
value (arguments after the member name are passed to the superclass
constructor):
```matlab
classdef Bearing < uint32
    enumeration
        North (0)
        East  (90)
        South (180)
        West  (270)
    end
end
% Bearing.East converts to uint32(90); usable in arithmetic/comparison.
```
Enumeration classes may also declare regular `properties` and a constructor to
attach data to each member, and define `methods`. Use cases: type-safe named
constants, replacing magic numbers/strings, `mustBeMember`-style validation.

Source:
- https://www.mathworks.com/help/matlab/matlab_oop/enumerations.html

---

## 9. Packages / Namespaces (`+folder`)

A **namespace** is a folder whose name begins with `+`. It scopes the classes,
functions, and sub-namespaces it contains.

```
+myns/                      % namespace "myns"
    MyClass.m               % myns.MyClass
    myFunction.m            % myns.myFunction
    @HeavyClass/HeavyClass.m% myns.HeavyClass (class folder; @ adds nothing to path name)
    +inner/                 % nested namespace
        Widget.m            % myns.inner.Widget
```

Rules:
- **Fully-qualified names** use dots from the top-level namespace:
  `myns.MyClass(args)`, `myns.inner.Widget`, `myns.myFunction(x)`. References
  from *inside* a namespace member to another member also require full
  qualification (except after `import`).
- The **parent** of the top-level `+` folder must be on the MATLAB path. You do
  **not** (and cannot meaningfully) put the `+` folder itself on the path.
- `@ClassFolder` names do **not** contribute to the namespace path; only `+`
  folders do.
- Namespace functions take precedence over same-named static methods.

### `import` statement
```matlab
import myns.inner.Widget      % qualified: brings in one class/function
import myns.inner.*           % unqualified: brings in all members of the namespace
```
After import, members are referenced by their short name within that function's
scope. Imports apply to the function/script in which they appear.

Sources:
- https://www.mathworks.com/help/matlab/matlab_oop/namespaces.html
- https://www.mathworks.com/help/matlab/matlab_oop/importing-classes.html
- https://www.mathworks.com/help/matlab/ref/import.html

---

## 10. Common Base Classes / Mixins

### `matlab.mixin.Copyable`
A handle mixin that adds a public `copy` method producing a **shallow copy** of
a handle object (copies all non-`Dependent`, non-`Transient`, non-`NonCopyable`
properties). MATLAB does **not** recurse into contained handle properties —
those are copied by reference. For **deep copy**, override the protected
`copyElement(obj)` method and explicitly `copy()` nested handle objects.
`copy` also does not copy dynamic properties or listeners by default.
```matlab
classdef Node < matlab.mixin.Copyable
    ...
    methods (Access = protected)
        function cp = copyElement(obj)
            cp = copyElement@matlab.mixin.Copyable(obj); % shallow base copy
            cp.Child = copy(obj.Child);                  % deep-copy a handle prop
        end
    end
end
```

### `dynamicprops`
Subclass of `handle` allowing **per-instance** properties added at runtime:
```matlab
p = addprop(obj, 'DynoProp');   % returns a matlab.metadata.DynamicProperty
obj.DynoProp = 42;
p.NonCopyable = false;          % opt the dynamic prop into copy()
```
Dynamic properties have `NonCopyable = true` by default, so a `Copyable` +
`dynamicprops` object does not copy them unless you clear that flag.

Sources:
- https://www.mathworks.com/help/matlab/ref/matlab.mixin.copyable-class.html
- https://www.mathworks.com/help/matlab/matlab_oop/custom-copy-behavior.html
- https://www.mathworks.com/help/matlab/matlab_oop/dynamic-properties-adding-properties-to-an-instance.html
- https://www.mathworks.com/help/matlab/ref/dynamicprops.addprop.html

---

## Implementer Priority: Foundational vs. Advanced

**Foundational (implement first — the core object model):**
1. `classdef` parsing: `properties`/`methods` blocks, block attributes,
   constructor recognition (name = class, returns `obj`).
2. **Value vs. handle semantics** — the copy-vs-reference split drives the whole
   runtime; get this right before anything else. Handle = shared reference +
   `delete`; value = copy-on-assign + return-to-mutate.
3. Ordinary methods with explicit `obj` first arg, and dual dispatch:
   `obj.m(x)` ≡ `m(obj,x)`.
4. Property attributes: `Access`/`GetAccess`/`SetAccess`, `Constant`,
   `Dependent`, `Transient`; default values; `get.`/`set.` access methods.
5. Property validation `(size) Class {validators} = default` with the documented
   validation order.
6. Single inheritance, method overriding, `obj@Super(...)` constructor chaining,
   `Abstract` classes/methods.
7. Namespaces (`+folder`), fully-qualified names, `import`.

**Advanced (layer on later):**
8. Operator overloading, especially **`subsref`/`subsasgn`** and the `S`
   struct-array dispatch model + `numArgumentsFromSubscript` (subtle, but needed
   for custom container classes).
9. Events/listeners, `notify`, `addlistener`, observable properties (handle-only
   observer machinery).
10. Enumeration classes (esp. subclassing built-in types).
11. Multiple inheritance / handle mixins: `matlab.mixin.Copyable`
    (`copyElement` deep-copy), `dynamicprops` (`addprop`), `matlab.mixin.SetGet`.
12. Modular indexing mixins (`RedefinesParen`/`Dot`/`Brace`) as an alternative to
    raw `subsref`/`subsasgn`.
