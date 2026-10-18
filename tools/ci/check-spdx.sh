#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Fails when a tracked source file lacks the SPDX licence header (CLAUDE.md):
# `// SPDX-License-Identifier: Apache-2.0` on the first line of Rust,
# TypeScript and JavaScript files, `# SPDX-License-Identifier: Apache-2.0` on
# the first line of Python, shell and PowerShell files, or on the second after
# a shebang. Fails as well when git cannot list the files.
set -euo pipefail

spdx='SPDX-License-Identifier: Apache-2.0'

root=$(git rev-parse --show-toplevel)
cd "$root"
list=$(mktemp)
trap 'rm -f "$list"' EXIT
git ls-files -z -- '*.rs' '*.ts' '*.tsx' '*.mts' '*.cts' '*.js' '*.jsx' '*.mjs' '*.cjs' \
    '*.py' '*.sh' '*.ps1' >"$list"

checked=0
missing=()
while IFS= read -r -d '' file; do
    checked=$((checked + 1))
    first='' second=''
    { IFS= read -r first || true; IFS= read -r second || true; } <"$file"
    case $file in
    *.py | *.sh | *.ps1)
        header="# $spdx"
        [[ $first == "$header" || ($first == '#!'* && $second == "$header") ]] && continue
        ;;
    *)
        [[ $first == "// $spdx" ]] && continue
        ;;
    esac
    missing+=("$file")
done <"$list"

if ((checked == 0)); then
    echo 'No tracked source file found.' >&2
    exit 1
fi
if ((${#missing[@]} > 0)); then
    echo "Missing \"// $spdx\" (\"# $spdx\" in Python, shell and PowerShell) in:" >&2
    printf '%s\n' "${missing[@]}" >&2
    exit 1
fi
