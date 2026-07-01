# Repository Rules

- For PR review, merge, release, and other repository operations, default to the `origin` remote. Do not inspect or operate on `upstream` unless the user explicitly requests upstream work.
- For GitHub CLI operations, do not rely on implicit repository resolution when both `origin` and `upstream` exist. Use `gh repo set-default FingerCaster/aio-coding-hub` for this clone and prefer explicit `--repo` / `-R FingerCaster/aio-coding-hub` on `gh` commands that mutate state or inspect Actions, releases, PRs, or issues.
- Keep `upstream` fetch-only for normal work. Do not restore an `upstream` push URL unless the user explicitly requests upstream push access.
