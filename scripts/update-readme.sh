#!/bin/sh
set -eu

self="$(realpath "$0")"
scripts_dir="$(dirname "$self")"
project_dir="$(dirname "$scripts_dir")"

cd "$project_dir"

printf 'Updating %s/README.md...\n' "$project_dir" >&2
(
	printf '# dyngo: dynamic generic outparams\n\n'
	cargo readme --no-title | sed -Ef scripts/strip-md-links.sed
) > README.md

printf 'Done!\n' >&2
