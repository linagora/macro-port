# ADR-0010: Calls qualified by the VBA library

Date: 2026-10-22 · Status: proposed (to validate) · Decided by: coordinator of the night run (option b, then the `library` flag), written by the night agent of ticket #41, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC section 7 defines the `call` matcher as "a procedure, function or statement invoked by name, not preceded by `.`". The facts reader gave every name after a dot `CallSite::after_dot`, and listed no call at all for the statement form `VBA.Shell "calc"`, where a name after a dot has no argument list. But the functions of the VBA library are reached by their bare name and through the library and its modules: `VBA.Shell("calc")`, `Interaction.Shell("calc")` and `VBA.Interaction.Shell("calc")` call the function that `Shell("calc")` calls. An engine that reads `after_dot` as "preceded by `.`" would miss all four spellings, for each `call` rule that names a function of the library: `sec.shell`, `sec.sendkeys`, `sec.file-delete`, `sec.callbyname` and the family rules such as `win.shell`, if the library holds their functions (see the names below).

## Decisions

**"Not preceded by `.`" means "not a member of an object".** A name qualified by names of the VBA library only is the function itself, `Shell(x)` written in full.

**The reader flags it, and `after_dot` stays true.** `CallSite` gains `library: bool`, true when every name before the call is a name of the library. `VBA.Shell(x)`, `VBA.Interaction.Shell(x)`, `Interaction.Shell(x)` and the statement form `VBA.Shell "calc"` give `{ name: "Shell", after_dot: true, library: true }`; `ws.Range(x)`, `o.Run(x)`, `Foo.VBA.Shell(x)` and `Application.SendKeys(x)` give `library: false`. `after_dot` keeps telling the source: the call graph reads a call with `after_dot` false as a bare mention, which resolves to the procedure of that name in the module of the statement or to one that every module can call (`Mention::Bare` in `vba/callgraph.rs`), and `VBA.Shell` is not the `Shell` of the project. The engine input maps `after_dot && !library` to "preceded by `.`" (the adapter of M3, `analyze/input.rs`), so the matcher keeps the wording and the contract of the SPEC.

**The names.** `VBA` and the eight modules of its standard library, in any case: `Conversion`, `DateTime`, `FileSystem`, `Financial`, `Information`, `Interaction`, `Math`, `Strings`. `Constants` is out: it holds constants, no function. The list comes from the brief of the coordinator, and my memory of the VBA type library says that it holds `Shell`, `SendKeys`, `CallByName`, `Kill` and `RmDir`; no file of this repository says either, for it holds no MS-VBAL text (a hypothesis to check). The two narrower lists of the reader stay as they are: `VBA` and `Strings` decide which `Chr` and `StrReverse` fold, `VBA` and `Interaction` which `CreateObject` names a ProgID, and `Math.Chr(65)` must not fold. The calls take the wide list because a rule names the function, not the module.

**The other forms.** Without an argument list, `VBA.Shell "calc"` and `Call VBA.Shell` are calls as `Shell "calc"` is, and `VBA.Shell = 1` is not, as `Dir = 1` is not. A `With VBA` block qualifies its dots (`.Shell "calc"`). A `!` qualifies nothing, and a name with an argument list (`VBA(1).Shell`) is no qualifier. In a part of a statement nested deeper than 64 levels, which is read flat, the qualifiers are read back from the tokens before the call, in linear time; the object of a `With` block is not followed there.

## Alternatives rejected

- **(a) The matcher joins `Facts::calls` and `Facts::chains`.** A call has no link to its chain, each consumer would carry the join, and the statement form has no call to join.
- **(c) Chain patterns in `rules/security.yaml`** (`VBA.Shell`, `VBA.Interaction.Shell`, `Interaction.Shell`...). One pattern per function and per spelling, a list that never closes, and a rules change (version bump, A4/A5 report) for a gap of the reader. The family rules would need the same.
- **`after_dot: false` for these calls.** The first form of the decision. The call graph would take `VBA.Shell` for the `Shell` of the project.
- **Every name after a dot is a call by name.** `ws.Cells` would match the rules that name `Cells`: the dot rule exists to prevent it.

## Consequences

- The reader goes by names. A module, class or variable of the project called `Math`, `Strings` or `Information` is taken for the library, and `Math.Kill x` is a call of `Kill`: an over-report, never a miss. A module of the library missing from the list is read as an object, and its functions are missed.
- The corpus does use these calls, contrary to the brief: the reader flags 100 statements of the text samples (98 in `11-05`, one each in `06-03` and `13-06`) and 300 of Excel's exports of the same three, all spelled `VBA.`, none with the name of a security rule (`Mid`, `CInt`, `Replace`, `Trim`...). A test checks them against a crude reader. A throwaway patch of the calibration prototype, which counts a call after a library qualifier as a call by name, leaves A4 (top-1 52/74, top-2 65/74) and A5 (17/17, every signal the same) as they are, and moves the scores of 2 of the 80 files: `11-05` (`03-cleaning` 14 to 55, which replaces `08-udf` as its second family) and `13-06` (`03-cleaning` 9 to 10). Those are prototype figures; the Rust ones come with the M3 adapter.
- The library constants (`VBA.vbCr`) are listed as calls, as `vbCr` alone is: the reader cannot tell a constant from a function.
- Still open, and unchanged: `With VBA.Interaction` then `.CreateObject("WScript.Shell")` gives no ProgID, and `.Chr(65)` in `With VBA` does not fold, for those two paths do not read the object of a block. `Application.SendKeys "x"` lists no call and `Application.SendKeys("x")` lists a call after a dot, so `sec.sendkeys` matches neither, for no rule holds the chain `Application.SendKeys`. Both are for the owner to decide.
