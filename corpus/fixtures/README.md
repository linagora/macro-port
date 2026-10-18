# Binary fixtures

- `generated/`: workbooks built by `tools/fixtures/Build-Fixtures.ps1` (run once on Windows with Excel), plus `*.expected.*` module exports used as reference sources. Described in `fixtures.yaml`.
- `malformed/`: broken files derived from `generated/` by `tools/fixtures/make_malformed.py` (deterministic, can run in CI). Described in `malformed/expected.yaml`.

Both folders are empty in the starter kit: see `docs/CORPUS-PLAN.md`, items 1 to 5.
