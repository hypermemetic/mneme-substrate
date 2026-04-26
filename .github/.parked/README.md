# Parked: workflows from upstream substrate

The inherited `ci.yml` was moved here because the OAuth token in use during the
Phase 0 push lacked GitHub's `workflow` scope, which is required to create or
update workflow files. To restore CI, either (a) re-grant `workflow` scope to
the gh CLI and `mv .github/.parked/workflows-from-upstream .github/workflows`,
or (b) write a fresh CI workflow tailored to mneme-substrate (probably the
right move — the upstream CI was tuned to plexus-substrate's release process).

Logged as LOG-9 in mneme/ISSUES.md.
