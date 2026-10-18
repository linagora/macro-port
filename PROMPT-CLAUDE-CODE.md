# Première commande pour Claude Code

À coller dans Claude Code, ouvert à la racine du dépôt `macro-port`.

---

Tu travailles sur Macro-Port, un analyseur open source de macros VBA Excel. Commence par lire, dans cet ordre : `CLAUDE.md`, `docs/SPEC-LOT1.md`, `docs/ACCEPTANCE.md`, `docs/adr/0001-framing-decisions.md`, `schema/report.schema.json`, `rules/*.yaml` et `corpus/README.md`.

Ensuite, sans écrire de code fonctionnel :

1. Propose-moi un plan d'implémentation du lot 1, découpé en demandes de fusion (pull requests) courtes. Pour chacune, donne l'objectif, les fichiers touchés, les tests ajoutés et les critères d'acceptation visés.
2. Liste les points de la spécification que tu trouves ambigus, incomplets ou contradictoires, avec ta proposition pour chacun.
3. Propose le choix des crates (conteneurs CFB et zip, YAML, i18n, tests), avec la licence et l'état de maintenance de chacune, sous forme d'ADR `docs/adr/0002-dependencies.md`.

Attends ma validation avant de commencer la première demande de fusion. Elle mettra en place le squelette du workspace Cargo, la CI (fmt, clippy, tests, cargo deny), le chargement et la validation des règles YAML (`macro-port rules check`) et la validation des JSON par le schéma. Pas encore d'extraction.

Si les classeurs binaires de `corpus/fixtures/generated/` ne sont pas encore présents, enchaîne les jalons M2 et M3 sur le corpus texte avant M1. Garde M5 (WebAssembly et mini-application web) pour la fin.
