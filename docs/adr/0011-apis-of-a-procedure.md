# ADR-0011: The APIs that a procedure uses

Date: 2026-10-22 · Status: proposed (to validate) · Decided by: night agent for ticket #43, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

The report schema describes `procedures[].apis` as "Object model members, chains, functions and ProgIDs used, normalised (e.g. Range.Interior, CreateObject:Scripting.Dictionary)", an array of `{ name, count }`. SPEC M2 item 3 lists what the structural parse reads in a procedure: call sites, member chains, `With` resolution, and the ProgIDs of `CreateObject` and `GetObject`. The statement reader gives the chains with the `With` objects resolved and the arguments dropped, the names invoked, and the ProgIDs as they fold. It reads one module at a time, and no declaration of a type.

The SPEC and the schema leave open:

- what "normalised" means: letter case, order, and what an argument does to a name;
- whether a name that stands alone is an API: `Shell` and `Trim` are functions of VBA, `i`, `ws` and `Helper` are a variable and a procedure of the project, and the facts list all of them as calls;
- how a ProgID is written beyond the one example, and what to write when the statement does not show which function created the object;
- what the list becomes when a hostile file makes it huge, and how a procedure that uses nothing differs from one whose list is not given;
- whether the rules in use change the list.

## Decisions

**Three kinds of name, one list.**

- A member chain is its names joined by dots, with the arguments of its calls left out and the object of a `With` block put before the names that continue it: `Workbooks.Open`, and `Range.Interior.Color` for `.Interior.Color = 255` inside `With Range("A1")`. Every chain that the facts list is an API, its first name included when that name is a variable (`ws.Cells.Clear`): the front end knows no type and this ADR does not guess one. A name in brackets is one element up to the closing bracket, whatever it holds.
- A ProgID is `Function:ProgID`, with the function that created the object and the class as it folds: `CreateObject:Scripting.Dictionary`, `GetObject:Word.Application`. The facts list the ProgIDs and the calls apart, and a call whose class does not fold gives no ProgID. The function is told from the calls of the statement that are to the library of VBA: a call with no dot before it, or one that a library name qualifies. The method of an object (`app.CreateObject`) is none, and gives no ProgID. When the statement holds as many such calls of the two functions as ProgIDs, each ProgID takes the function at its own position. When it holds more calls, the function is the one that all of them call, if they call one, and else the label is the neutral `ProgID` (`IIf(c, CreateObject(n), GetObject(, "A.B"))` gives `ProgID:A.B`).
- A function called by name is its name as the code spells it (`Shell`, `Trim`). It is listed when it has no dot before it and either a `call` matcher of the rules in use names it or the module declares it with `Declare`. A function that the VBA library qualifies (`VBA.Strings.Replace(x)`, which ADR-0010 flags `library` while `after_dot` stays true) has a dot before it: the chain of its statement names it, so it is listed as that chain and not again by its name. A call with arguments that a dot follows (`Intersect(a, b).Address`) is both a call and the start of a chain in the facts. The chain is listed and the call is not, once for each chain of the statement that starts with its name.

**The rules in use decide which functions are listed, so `--rules` changes `apis`.** A name that stands alone is a variable, a constant, a procedure of the project or a function, and a statement does not say which. A name is kept when a rule loaded with the analysis cares for it as a function (the `call` matchers of the families and of the security signals, in any case) or when the module declares it. The tool keeps no list of the functions of VBA of its own. `tool.rules_version` names the version that the three files share, not their content: two runs with other rules of the same version can list other functions.

**What the statement reader gives is taken as it is.** A chain that starts with a dot continues the object of the enclosing `With` block when that object is a chain of names, and a block inside another continues it (`Range.Font.Bold`). Where the reader cannot, the names after the dot are all that the chain holds, and one name is no chain: the object is no name (`With New Collection`), a `)` lies before the dot, more blocks are open than it follows, or the object is too large to copy. A call that opens a block (`With Intersect(a, b)`) is a use of the function, and the chains of the block start with its name: a call of the same function in a statement of the block (`.Value = Intersect(c, d)`) goes with the chain that the statement holds and is not listed, one use fewer. A statement nested too deep (`vba.statement.too_deep`) is read flat from where it passes the limit: no chain and no ProgID is found after that, and a function that stands alone there is listed if a rule names it.

**Spelling, order and count.** VBA ignores letter case, so `Range.Value` and `range.value` are one entry, spelled as the first use spells it. The entries are in the order of their names lowercased, whatever the order of the statements, so that one text gives one list. `count` is the number of uses in the procedure: a chain met twice counts twice. The statements outside the procedures belong to no list.

**Bounds.** A list holds at most 1,000 names, and the lists of a file weigh at most 4 MiB together (an entry weighs 64 and the bytes of its name and of the lowercased copy that keys it). A list that would pass a bound is left out of the report (`apis` absent), not cut, since a short list would read as a whole one. It gives back to the file what it had taken, so the lists after it keep their room. The module that holds such procedures gets one warning of the file, `vba.apis.left_out`, with the count of the procedures, the line where the first starts and the two bounds. Its status is `unchanged`: the module is read, and no failure follows. A procedure that uses nothing has `apis: []`, and `apis` absent says that the list was left out.

**One walk.** The lists are fed by the pass that reads the statements of a module once (`Statements::read`), with the ProgIDs of the file and the statements nested too deep: one visitor, the facts of a statement handed over by value and dropped after it. No stage reads the statements again.

## Alternatives rejected

- **List every name that stands alone.** Variables, constants and procedures of the project would fill the lists, and nothing in the facts tells them from the functions. An inventory of APIs would be mostly noise.
- **A vocabulary that does not depend on the rules.** A list of the functions of VBA in Rust breaks the rule that families and signals are data and the core holds matchers only. A new `rules/apis.yaml` is a new contract (schema, checks, version, A4/A5 report) for names that the `call` matchers already hold, and a second list to keep in step with the first. It would make `apis` stable under `--rules`. To revisit if an inventory must list functions that no rule names, such as `MsgBox`.
- **List a library-qualified call by its bare name and drop its chain.** The chain cannot be tied to its call without the list of library names, which the statement reader owns; copying it here is a second list. **List both:** one use would count twice. The same goes for a call that starts a chain: the chain says more, and it is the one kept.
- **Tag each ProgID with its function in the facts.** It would end the reading of the position and the neutral label. It changes `Facts`, a public type that every stage reading statements takes, and its tests, for a case that the corpus never shows: of its 34 ProgIDs none has the neutral label. To revisit if the label shows in real code.
- **Keep the most frequent or the smallest spelling of a name.** The first use is deterministic and needs no second pass.
- **Cut a list at the bound.** It would read as a whole list. The bound is far above real code, so the cases that reach it are the ones to flag.
- **Leave a list out without a word.** `apis` absent would then mean either that the list was not given or that the file passed a bound, and SPEC section 3 item 2 asks for a typed error where a limit is enforced.
- **Resolve a variable to its type** (`ws` to `Worksheet`). It needs the declarations and the object model, which the front end does not read.

## Consequences

- **A variable can root a chain, and stays in clear.** This answers a question that ADR-0007 (redaction for sharing, on the open PR #137) leaves to this one. The samples give `targetWorksheet.Columns.Cells.End.Offset.Row` and `foundMatchingallOnHandListSku.End.Offset.value`. ADR-0007 hashes the first part of a name only when the project defines it as a module, a stream or a procedure, so a variable stays under `--redact-identifiers`, and so do the names of the routines that a module declares (`GetTickCount`, or the routine of an in-house library) and the ProgIDs. If the name of a variable can identify an organisation, ADR-0007 needs a rule for them, and the owner decides.
- **`apis[].name` holds text of the file.** A name in brackets is one element up to the first `]` and may hold spaces, dots, quotes or tags: `[a b <i>].d` gives the name `a b <i>.d`. It is an untrusted string like any other that comes from the file (CLAUDE.md): the renderers escape it, and the redaction of names has to take it as it is.
- A routine that another module declares is not known to the module that calls it, so it is not listed. A `Declare` under `#If` counts whichever branch holds it, as the count of declarations does.
- A `CreateObject` whose class does not fold, a `GetObject` with a path, and the method of another object (`app.CreateObject`, which is a chain) give no ProgID.
- A function that no rule names (`MsgBox`) is not listed, nor a global member used with no dot after it (`Range("A1")` alone). A `call` matcher in the rules lists it.
- `Replace(x)` and `VBA.Strings.Replace(x)` are two entries, and the names of the language (`Err.Number`, `Debug.Print`) are listed like those of the object model: the front end has no list of what belongs to the host.
- A name in brackets that holds a dot (`[a.b].c`) and the chain `a.b.c` are one entry.
- On the 80 samples of the corpus the lists hold 2,601 entries (2,203 chains, 364 functions, 34 ProgIDs) over 428 procedures, 54 of them empty. None of the 364 functions starts a chain, and none of the lists was left out. The longest list has 51 entries and the longest name 60 characters, so the bounds are far above real code.
