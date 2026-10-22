# ADR-0012: The procedures that a procedure calls

Date: 2026-10-22 · Status: proposed (to validate) · Decided by: night agent for ticket #43, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

The report schema describes `procedures[].calls` as "Procedures of the same project called by this one", an array of strings. SPEC M2 item 5 asks for the call graph of the project, "by name, best effort", including `Application.Run "Name"` and `CallByName obj, "Name"` when the name folds to a literal, and names the entry points that decide `possibly_unused`. `vba::CallGraph` reads names a module and a statement at a time and answers once the project is read. Until now nothing fed it, and every report left `calls` out.

The SPEC and the schema leave open:

- how a procedure is named in a list, since two modules may declare one name and the report has no key for a procedure;
- what the lists become when a module of the file could not be read, and which modules count as unread;
- what a list becomes when it passes a bound, and whether the file says so.

The SPEC is not silent on one point, and this ADR departs from it. M2 item 5 and section 7 (`possibly_unused`, `parse_recovered`: "computed by M2, used by traits") give `possibly_unused` to M2, and the graph can tell it, and `is_entry_point`, as well as `calls`. See "Only `calls` is filled".

## Decisions

**A list of names, each once, in byte order.** `calls` holds the procedures of the project that this one calls, as the project declares their names (VBA ignores letter case, so when modules spell a name differently the smallest spelling in byte order is the one), each once and sorted. A list says which procedures, not how often and not where: two modules that declare one name give one entry. Recursion is no call: a procedure that refers to its own name lists nothing for it, unless the name stands for the procedures of other modules too, as a name after a dot does. A procedure that calls nothing has `calls: []`, and `calls` absent says that the list is not known. One text gives one list: the order of the modules changes the order of the procedures in the report and nothing else.

**What counts as a call is the reading of `vba/callgraph.rs`**, which its module documentation states and this ADR does not repeat. In short, a bare name reaches the procedures of its own module and the public ones of the standard modules, and a name after a dot, a member, and the name that a statement gives to `Run`, `CallByName`, `OnTime`, `OnKey` or `OnAction` reach every procedure of that name. A call that the VBA library qualifies (ADR-0010) is a name after a dot. A name is not told from a variable or a property of the same name: the graph errs on the side of a procedure being used, and a list may hold a procedure that the code does not call.

**A module that is not read leaves the calls of the whole file unknown.** A module with no source, one that a limit refuses (`vba.too_many_lines`, `vba.too_many_procedures`) and one whose source is suspected to be lost (`source_missing_suspected`) may declare any procedure and call any. A list of the other modules would read as whole, and a procedure that nothing seems to call could be called from there. The front end places each module of the file in the graph, the unread ones through `CallGraph::skip_module`, so that the answer of the graph lines up with the modules of the report. From then on the graph answers no `calls` and no `possibly_unused`, as it does past a limit of its own, and keeps `is_entry_point`, which does not depend on what was read. The procedures of the modules that were read are measured all the same; only `calls` is left out. The modules that the rule engine of M3 refuses by a room of its own are to be placed by the same method.

**One walk.** The graph is fed by the pass that reads the statements of a module once (`Statements::read`, ADR-0011): the facts of a statement go to the ProgIDs of the file, to the lists of APIs, to the count of the statements nested too deep, and to the graph, and are dropped after it. No stage reads the statements again. The statements outside the procedures belong to no procedure and are the call of none.

**Only `calls` is filled, which departs from the SPEC.** SPEC M2 item 5 and section 7 have M2 compute `possibly_unused` from the entry points. Here `is_entry_point`, `is_event_handler` and `possibly_unused` stay absent. The procedures that the host starts by itself (event handlers, auto-exec procedures) are decided by the structural predicates of the rule engine (SPEC section 7), which M3 wires, and the core keeps no copy of them in `vba/`. Filled without them, the fields would be wrong for the procedures that matter most (`Workbook_Open` would be possibly unused), and a missing value is not a zero. The wiring of M3 gives the graph those procedures (the `started` argument of `CallGraph::add_module`) and fills the three fields, and `summary.totals.possibly_unused_procedures`, from the same result. The calls do not depend on the entry points, so this change gives none.

**Bounds.** A procedure lists at most 1,000 procedures and the lists of a file hold 500,000 together. A list that would pass a bound is left out (`calls` absent), not cut, and the others keep theirs. The graph keeps at most 1 MiB of distinct names, reads at most 4,000,000 mentions and keeps at most 2,000,000 references (`LIMITS` in `vba/callgraph.rs`); past one of these it answers nothing for the whole project, as for an unread module.

**A list that a bound leaves out is told.** When lists are left out by a bound and every module was read, the file gets one error, `vba.calls.left_out`, and keeps its status. As `vba.apis.left_out` does for the lists of APIs (ADR-0011), the error gives the count of the procedures that have no list, and the module and the line of the first. SPEC section 3 item 2 asks for a typed error where a limit is enforced, and without one a reader finds `calls` absent with no cause given. The error is about the file, not about each procedure, so a generated file that passes a bound gives one error and not thousands. When a module was not read, the lists are left out for that reason, and that module has its own error or signal: the file says nothing more.

## Alternatives rejected

- **List the calls of the modules that were read.** The lists would be true of the visible source and would read as whole. A module that hides its procedures is the case where a reader most needs to know that the project is not whole.
- **Qualify the names (`Module1.Helper`).** It would tell two procedures of one name apart, but a call by a bare name or after a dot does not say which one is meant, so the qualifier would be a guess, and every other procedure name of the report is bare.
- **Point at the procedures by their position in the report.** It breaks as soon as a consumer sorts, filters or redacts the procedures, and a list of names is what the schema describes.
- **Fill the entry points with the calls.** The graph tells the public procedures of the standard modules and the events of a class, and it can only be told the event handlers and the auto-exec procedures. A copy of those predicates in `vba/` breaks the rule that the rules are data, and leaving them out gives false values.
- **Feed the graph in a second pass, or keep the facts.** The facts of a statement weigh a few dozen times its text. Reading the statements twice doubles the heaviest stage, and keeping the facts takes memory that grows with the file.
- **A stomped module is read for what it shows.** Its visible source holds no procedure, its compiled part does. Counting it as read would give lists that look whole for a file that hides code.
- **One error for each procedure whose list is left out, or none.** The first gives thousands of errors for a generated file that passes a bound, the second leaves `calls` absent with no cause given.

## Consequences

- **Only a generated file reaches a bound.** Read as one project, the 80 samples of the corpus hold at most some 5,500 distinct identifiers, 50 KB of names against a bound of 1 MiB, and the longest list holds 8 procedures against 1,000. The tests of `vba.calls.left_out` build their text. The error names the lists of calls only: `possibly_unused` is left out for the same causes, and the wiring of M3 that fills it is to extend the error.
- **A text module is a project of its own.** `analyze` of a `.bas`, `.cls` or `.frm` file builds the graph of that module alone, so a call to a procedure that another file declares is not listed.
- **A name is not a type.** `fso.BuildPath(a, b)` lists the `BuildPath` of a module that declares one, whatever `fso` is.
- **A name is not told from a variable or an enum member either.** In the corpus, `CopyVariant` lists `value`, its own parameter, because its class declares `Property Get value`, and the `Property Set Picture` lists `XLPicture`, the `xlPicture` of the `XlCopyPictureFormat` that it passes, because the class declares `Property Set XLPicture` and VBA ignores letter case. A list may hold a name that no call reaches.
- **The names of `calls` are procedure names.** The redaction of the reports (M4) hashes the module and procedure names with `--redact-identifiers` (SPEC section 8) and has to hash those of `calls` with the same key, or the redacted report gives away what the hashed names hide. This change adds the field and does not redact it.
- The 80 samples of the corpus hold 428 procedures: 199 call 1 to 8 procedures (113 call one) and 229 call none. None of the lists is absent.
- Excel's editor changes the letter case of an identifier to that of its declaration, so a workbook and the text it was exported from may spell a name of the lists of APIs differently (`XLPicture` and `xlPicture`). The names of `calls` are the declared ones and agree. The corpus test that builds each sample as text and as workbooks now compares `calls`, and `apis` in lowercase.
