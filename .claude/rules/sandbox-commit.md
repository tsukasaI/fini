# Committing from the sandbox

- The pre-commit hook runs prek, which opens `~/.cache/prek/prek.log` before any check runs. The Bash sandbox cannot write there, so a sandboxed `git commit` fails with `failed to open file ... prek.log: Operation not permitted`. Stage the files, then hand the exact `git commit` command to the user.
