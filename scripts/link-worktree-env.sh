#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "link-worktree-env: $*" >&2
  exit 1
}

if (( $# != 1 )); then
  fail "usage: scripts/link-worktree-env.sh <worktree-path>"
fi

readonly repository_root="$(git rev-parse --show-toplevel)"
readonly common_git_directory="$(git rev-parse --path-format=absolute --git-common-dir)"
readonly main_worktree="${common_git_directory%/.git}"

[[ "$repository_root" == "$main_worktree" ]] \
  || fail "run this script from the main worktree: $main_worktree"

[[ -d "$1" ]] || fail "worktree path does not exist: $1"
readonly worktree_path="$(cd "$1" && pwd -P)"
readonly worktree_common_git_directory="$(git -C "$worktree_path" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" \
  || fail "target is not a Git worktree: $worktree_path"

[[ "$worktree_path" != "$main_worktree" ]] || fail "target must not be the main worktree"
[[ "$worktree_common_git_directory" == "$common_git_directory" ]] \
  || fail "target does not belong to the main worktree repository: $worktree_path"

linked_count=0
for env_file in "$main_worktree"/.env "$main_worktree"/.env.*; do
  [[ -f "$env_file" ]] || continue
  [[ "$env_file" != *.example ]] || continue

  link="$worktree_path/$(basename "$env_file")"
  if [[ -L "$link" ]]; then
    [[ "$(readlink "$link")" == "$env_file" ]] \
      || fail "existing symlink does not point to the main worktree: $link"
  elif [[ -e "$link" ]]; then
    fail "refusing to replace existing file: $link"
  else
    ln -s "$env_file" "$link"
  fi

  [[ -r "$link" ]] || fail "linked environment file is not readable: $link"
  ((linked_count += 1))
done

(( linked_count > 0 )) || fail "no runtime environment files found in $main_worktree"
echo "Linked and verified $linked_count runtime environment files in $worktree_path"
