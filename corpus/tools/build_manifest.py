#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Agrege les manifest.json de chaque famille, verifie le corpus et produit manifest.json / manifest.csv a la racine."""
import json, glob, os, csv, hashlib, collections, sys

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(root)
entries, issues = [], []
for m in sorted(glob.glob("families/*/manifest.json")):
    fam = m.split("/")[1]
    for e in json.load(open(m, encoding="utf-8")):
        e["path"] = f"families/{fam}/{e['file']}" if e.get("file") else None
        entries.append(e)

for k, v in collections.Counter(e["id"] for e in entries).items():
    if v > 1:
        issues.append(f"id en double : {k}")
listed = {e["path"] for e in entries if e["path"]}
for f in glob.glob("families/*/*"):
    if not f.endswith("manifest.json") and f not in listed:
        issues.append(f"fichier non reference : {f}")
seen = collections.defaultdict(list)
for e in entries:
    p = e["path"]
    if not p:
        continue
    if not os.path.exists(p):
        issues.append(f"fichier manquant : {p}")
        continue
    txt = open(p, encoding="utf-8", errors="replace").read()
    if not txt.startswith("' Source:"):
        issues.append(f"en-tete absent : {p}")
    body = "\n".join(l for l in txt.splitlines() if not l.startswith("' "))
    seen[hashlib.sha1(body.encode()).hexdigest()].append(e["id"])
issues += [f"contenu en double : {v}" for v in seen.values() if len(v) > 1]

json.dump(entries, open("manifest.json", "w", encoding="utf-8"), ensure_ascii=False, indent=2)
cols = ["id", "family", "title", "status", "license", "author", "loc", "expected_target",
        "migration_difficulty", "secondary_families", "key_apis", "blockers", "source_url", "path", "notes"]
with open("manifest.csv", "w", newline="", encoding="utf-8") as fh:
    w = csv.writer(fh)
    w.writerow(cols)
    for e in entries:
        w.writerow(["; ".join(e.get(c) or []) if isinstance(e.get(c), list) else e.get(c) for c in cols])
print(f"{len(entries)} entrees, {sum(1 for e in entries if e['path'])} fichiers")
print("\n".join(issues) or "aucune anomalie")
sys.exit(1 if issues else 0)
