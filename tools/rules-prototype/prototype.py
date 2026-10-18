#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Rules calibration prototype for Macro-Port.

NOT the product. A deliberately simple, regex-based approximation of the Lot 1 rule engine,
used only to sanity-check rules/*.yaml against the corpus ground truth and to calibrate
thresholds before the Rust implementation exists. Do not port this code: implement the
real lexer/parser described in docs/SPEC-LOT1.md.

Usage: python3 -I prototype.py <kit_root> [--report out.json] [--verbose]
"""
import fnmatch, json, re, sys, collections, os, hashlib

import yaml

SEV = {"info": 0, "low": 1, "medium": 2, "high": 3}


def strip_line(line):
    """Return (code_without_strings_and_comments, string_literals, comment)."""
    code, strings, buf, i, in_str = [], [], [], 0, False
    while i < len(line):
        c = line[i]
        if in_str:
            if c == '"':
                if i + 1 < len(line) and line[i + 1] == '"':
                    buf.append('"'); i += 2; continue
                in_str = False; strings.append("".join(buf)); buf = []; code.append('""')
            else:
                buf.append(c)
        else:
            if c == '"':
                in_str = True
            elif c == "'":
                return "".join(code), strings, line[i + 1:]
            else:
                code.append(c)
        i += 1
    s = "".join(code)
    m = re.match(r"^\s*Rem\b(.*)$", s, re.I)
    if m:
        return "", strings, m.group(1)
    return s, strings, ""


def logical_lines(src):
    out, acc, start = [], "", None
    for n, raw in enumerate(src.splitlines(), 1):
        if raw.startswith("' ") and n < 12 and ("Source:" in raw or "License:" in raw or "Family:" in raw or "Author:" in raw or "Retrieved:" in raw or "Synthetic:" in raw):
            continue  # corpus header
        if start is None:
            start = n
        if raw.rstrip().endswith(" _"):
            acc += raw.rstrip()[:-1] + " "
            continue
        acc += raw
        out.append((start, acc)); acc, start = "", None
    if acc:
        out.append((start, acc))
    res = []
    for n, l in out:
        code, strings, comment = strip_line(l)
        res.append({"line": n, "raw": l, "code": code, "strings": strings, "comment": comment})
    return res


PROC_START = re.compile(r"^\s*(?:(Public|Private|Friend)\s+)?(?:Static\s+)?(Sub|Function|Property\s+(?:Get|Let|Set))\s+(\w+)", re.I)
PROC_END = re.compile(r"^\s*End\s+(Sub|Function|Property)\b", re.I)


def module_type(path, src):
    ext = os.path.splitext(path)[1].lower()
    if ext == ".frm" or re.search(r"^Begin\s+\{[0-9A-F-]+\}", src, re.M | re.I):
        return "form"
    if ext == ".cls":
        if re.search(r"Attribute VB_Base|ThisWorkbook|Private Sub Workbook_|Private Sub Worksheet_", src):
            return "document"
        return "class"
    return "standard"


def split_procs(lines):
    procs, cur, module_lines = [], None, []
    for l in lines:
        m = PROC_START.match(l["code"])
        if m and cur is None and not re.match(r"^\s*(Public\s+|Private\s+)?Declare\b", l["code"], re.I):
            cur = {"name": m.group(3), "kind": m.group(2).lower(), "vis": (m.group(1) or "default").lower(), "lines": [l], "start": l["line"]}
            continue
        if cur is not None:
            cur["lines"].append(l)
            if PROC_END.match(l["code"]):
                cur["end"] = l["line"]; procs.append(cur); cur = None
        else:
            module_lines.append(l)
    if cur is not None:
        cur["end"] = cur["lines"][-1]["line"]; procs.append(cur)
    return procs, module_lines


def progids(lines):
    out = []
    for l in lines:
        for m in re.finditer(r"\b(CreateObject|GetObject)\s*\(\s*(\"\"|[^,)]*)", l["code"], re.I):
            # strings are blanked in code; recover from literal list in order of appearance
            pass
        if re.search(r"\b(CreateObject|GetObject)\s*\(", l["code"], re.I):
            folded = fold_literals(l["raw"])
            cands = set(l["strings"]) | set(folded)
            out.extend((s, l) for s in cands if re.match(r"^[A-Za-z][\w.]*\.[\w.]+$", s))
    return out


def fold_literals(raw):
    """Fold CreateObject/GetObject arguments made of literals, Chr(n) and StrReverse(literal) joined by &."""
    res = []
    for m in re.finditer(r"\b(?:CreateObject|GetObject)\s*\((.*?)\)\s*$|\b(?:CreateObject|GetObject)\s*\(([^)]*\))", raw, re.I):
        arg = (m.group(1) or m.group(2) or "")
        parts, ok = re.split(r"\s*&\s*", arg.strip()), True
        val = ""
        for part in parts:
            q = re.fullmatch(r'"((?:[^"]|"")*)"', part)
            c = re.fullmatch(r"Chr[WB]?\$?\s*\(\s*(\d+)\s*\)", part, re.I)
            r = re.fullmatch(r'StrReverse\s*\(\s*"((?:[^"]|"")*)"\s*\)', part, re.I)
            if q: val += q.group(1).replace('""', '"')
            elif c: val += chr(int(c.group(1)))
            elif r: val += r.group(1)[::-1]
            else: ok = False; break
        if ok and val: res.append(val)
    return res


def wordlist(p):
    return p if isinstance(p, list) else [p]


def match_signal(sig, lines, ctx):
    """Return list of (line, snippet) hits."""
    hits, m = [], sig["match"]
    pats = wordlist(sig["pattern"])
    if m == "member":
        rx = re.compile(r"(?<![\w])(" + "|".join(map(re.escape, pats)) + r")\b(?!\s*:=)", re.I)
        for l in lines:
            for x in rx.finditer(l["code"]):
                hits.append((l["line"], l["raw"].strip()))
    elif m == "chain":
        alts = []
        for p in pats:
            parts = [re.escape(x) for x in p.split(".")]
            alts.append(r"\b" + r"(?:\s*\([^()]*\))?\s*\.\s*".join(parts) + r"\b")
        rx = re.compile("|".join(alts), re.I)
        for l in lines:
            if rx.search(l["code"]):
                hits.append((l["line"], l["raw"].strip()))
    elif m == "call":
        rx = re.compile(r"(?<![.\w])(" + "|".join(map(re.escape, pats)) + r")\$?\b(?!\s*[=.])", re.I)
        for l in lines:
            if rx.search(l["code"]) and not re.match(r"^\s*(Dim|Private|Public|Const)\b", l["code"], re.I):
                hits.append((l["line"], l["raw"].strip()))
    elif m == "progid":
        for s, l in ctx["progids_in"](lines):
            for p in pats:
                if p == "*":
                    if not ctx["progid_claimed"](s):
                        hits.append((l["line"], l["raw"].strip()))
                elif fnmatch.fnmatch(s.lower(), p.lower()):
                    hits.append((l["line"], l["raw"].strip()))
    elif m == "procedure_name":
        if ctx.get("proc") and re.search(pats[0], ctx["proc"]["name"], re.I):
            hits.append((ctx["proc"]["start"], ctx["proc"]["lines"][0]["raw"].strip()))
    elif m == "module_type":
        if ctx["module_type"] == pats[0]:
            hits.append((1, ctx["module_type"]))
    elif m == "reference":
        pass  # no references in text samples
    elif m == "regex":
        rx = re.compile(pats[0], re.I)
        field = "raw" if sig.get("text") == "raw" else "code"
        for l in lines:
            if rx.search(l[field]):
                hits.append((l["line"], l["raw"].strip()))
    elif m == "string":
        rx = re.compile(pats[0], re.I)
        for l in lines:
            if any(rx.search(s) for s in l["strings"]):
                hits.append((l["line"], l["raw"].strip()))
    elif m == "comment":
        rx = re.compile(pats[0], re.I)
        for l in lines:
            if rx.search(l["comment"]):
                hits.append((l["line"], l["raw"].strip()))
    elif m == "structural":
        hits = structural(pats[0], lines, ctx)
    return hits


def structural(name, lines, ctx):
    p = ctx.get("proc")
    if name == "event_handler":
        if p and (re.match(r"^(Worksheet|Workbook|Chart|App|Application|xlApp)_\w+$", p["name"], re.I) or
                  (ctx["module_type"] in ("class", "document") and any(p["name"].lower().startswith(v.lower() + "_") for v in ctx["withevents"]))):
            return [(p["start"], p["name"])]
    elif name == "auto_exec":
        if p and re.match(r"^(Auto_?Open|Auto_?Close|AutoExec|Workbook_(Open|Activate|BeforeClose))$", p["name"], re.I):
            return [(p["start"], p["name"])]
    elif name == "udf_candidate":
        if p and p["kind"] == "function" and p["vis"] in ("public", "default") and ctx["module_type"] == "standard":
            body = "\n".join(l["code"] for l in p["lines"])
            if not re.search(r"\.(Select|Activate|Copy|Delete|Insert|ClearContents)\b|\.Value\s*=|MsgBox|\.Font\.|\.Interior\.", body, re.I):
                return [(p["start"], p["name"])]
    elif name == "nested_loop_compare":
        depth, maxd, hit = 0, 0, None
        for l in lines:
            c = l["code"]
            if re.match(r"^\s*(For|Do)\b", c, re.I):
                depth += 1; maxd = max(maxd, depth)
            elif re.match(r"^\s*(Next|Loop)\b", c, re.I):
                depth = max(0, depth - 1)
            elif depth >= 2 and re.search(r"^\s*(If|ElseIf)\b.*(=|StrComp)", c, re.I):
                hit = (l["line"], l["raw"].strip())
        return [hit] if hit else []
    elif name == "chr_obfuscation":
        out = []
        for l in lines:
            if len(re.findall(r"\bChr[WB]?\$?\s*\(", l["code"], re.I)) >= 3 or re.search(r"\bStrReverse\s*\(\s*\"\"", l["code"], re.I):
                out.append((l["line"], l["raw"].strip()))
        return out
    elif name == "long_hex_or_base64_literal":
        return [(l["line"], "<literal>") for l in lines if any(len(s) >= 200 and re.fullmatch(r"[A-Za-z0-9+/=]+|[0-9A-Fa-f]+", s) for s in l["strings"])]
    return []


def analyse(path, rules, sec):
    src = open(path, encoding="utf-8", errors="replace").read()
    lines = logical_lines(src)
    mtype = module_type(path, src)
    procs, mod_lines = split_procs(lines)
    withevents = [m.group(1) for l in mod_lines for m in [re.search(r"\bWithEvents\s+(\w+)", l["code"], re.I)] if m]
    all_progids = [s.lower() for s, _ in progids(lines)]
    claimed_patterns = [p.lower() for f in rules["families"] for s in f["signals"] if s["match"] == "progid" for p in wordlist(s["pattern"]) if p != "*"]
    ctx = {"module_type": mtype, "withevents": withevents, "progids_in": progids,
           "progid_claimed": lambda s: any(fnmatch.fnmatch(s.lower(), p) for p in claimed_patterns)}
    default_cap = rules["scoring"]["default_max_hits_per_procedure"]
    if not procs:  # module with only declarations
        procs = [{"name": "(module)", "kind": "sub", "vis": "default", "lines": mod_lines, "start": 1, "end": len(lines)}]
    file_scores = collections.Counter()
    proc_out = []
    for p in procs:
        ctx["proc"] = p
        scores, evid = collections.Counter(), collections.defaultdict(list)
        for fam in rules["families"]:
            for s in fam["signals"]:
                scope = s.get("scope", "procedure")
                target = mod_lines + p["lines"] if scope != "procedure" else p["lines"]
                h = match_signal(s, target, ctx)
                if not h:
                    continue
                cap = s.get("max_hits_per_procedure", default_cap)
                w = s.get("weight", 1) * (0.5 if scope != "procedure" else 1)
                scores[fam["id"]] += w * min(len(h), cap)
                evid[fam["id"]].append(s["id"])
        top = scores.most_common()
        primary = top[0][0] if top and top[0][1] >= rules["scoring"]["min_score"] else "unclassified"
        file_scores.update(scores)
        sigs = []
        for s in sec["signals"]:
            if s.get("requires_family") and s["requires_family"] not in scores:
                continue
            target = mod_lines + p["lines"] if s.get("scope") == "module" else p["lines"]
            if match_signal(s, target, ctx):
                sigs.append(s["id"])
        proc_out.append({"name": p["name"], "primary": primary, "scores": dict(scores), "signals": sigs})
    file_signals = sorted({x for p in proc_out for x in p["signals"]})
    for ch in sec.get("chains", []):
        def ok(term):
            if isinstance(term, str):
                return term in file_signals
            return any(ok(t) for t in term["any"])
        if all(ok(t) for t in ch["all"]):
            file_signals.append(ch["id"])
    ftop = file_scores.most_common(2)
    return {"path": path, "module_type": mtype, "procedures": proc_out,
            "file_primary": ftop[0][0] if ftop and ftop[0][1] >= rules["scoring"]["min_score"] else "unclassified",
            "file_top2": [f for f, _ in ftop], "file_scores": dict(file_scores), "signals": file_signals}


def main():
    root = sys.argv[1]
    verbose = "--verbose" in sys.argv
    rules = yaml.safe_load(open(os.path.join(root, "rules/families.yaml")))
    sec = yaml.safe_load(open(os.path.join(root, "rules/security.yaml")))
    gt_sec = yaml.safe_load(open(os.path.join(root, "corpus/ground-truth/security-expected.yaml")))
    slug2id = {f["corpus_slug"]: f["id"] for f in rules["families"]}
    manifest = json.load(open(os.path.join(root, "corpus/manifest.json")))
    per_fam = collections.defaultdict(lambda: [0, 0, 0])
    results, confusion = [], collections.Counter()
    sec_ok, sec_total, fp = 0, 0, collections.Counter()
    for e in manifest:
        if not e.get("path"):
            continue
        r = analyse(os.path.join(root, "corpus", e["path"]), rules, sec)
        results.append(r)
        if e["family"] == "14-patterns-securite":
            exp = set(gt_sec.get(e["id"], []))
            got = set(r["signals"])
            sec_total += len(exp); sec_ok += len(exp & got)
            if verbose and exp - got:
                print("MISSED", e["id"], exp - got)
            continue
        exp = slug2id[e["family"]]
        per_fam[exp][0] += 1
        if r["file_primary"] == exp:
            per_fam[exp][1] += 1
        if exp in r["file_top2"]:
            per_fam[exp][2] += 1
        else:
            confusion[(exp, r["file_primary"])] += 1
            if verbose:
                print(f"MISS {e['id']:6} exp={exp:20} got={r['file_primary']:20} top2={r['file_top2']}")
    n = sum(v[0] for v in per_fam.values())
    t1 = sum(v[1] for v in per_fam.values()); t2 = sum(v[2] for v in per_fam.values())
    print(f"Samples evaluated (families 01-13): {n}")
    print(f"Top-1 accuracy: {t1}/{n} = {t1/n:.0%} ; top-2: {t2}/{n} = {t2/n:.0%}")
    for f in sorted(per_fam):
        v = per_fam[f]
        print(f"  {f:22} n={v[0]}  top1={v[1]}  top2={v[2]}")
    print(f"Security ground truth (family 14): {sec_ok}/{sec_total} expected signals detected")
    if "--report" in sys.argv:
        json.dump(results, open(sys.argv[sys.argv.index("--report") + 1], "w"), indent=1)


if __name__ == "__main__":
    main()
