# Hyperbeam

## Tests

- `scripts/test.sh` runs the full gate that CI runs (`.github/workflows/test.yml`): web typecheck and build,
  docs build, clippy with `-D warnings`, and `cargo test --workspace` including the end-to-end tests.
  It starts a throwaway Postgres in Docker (`hyperbeam-test-pg`, port 5441) for them.
- `scripts/test.sh rust [cargo test args]` runs only Rust tests, e.g. `scripts/test.sh rust -p hyperbeam-server --test e2e`.
- Without `HYPERBEAM_TEST_DATABASE_URL` the e2e and master-key tests skip themselves and still report `ok`.
  A plain `cargo test` that passes has not run them; use the script.
- Docker on this machine is Docker Desktop with WSL integration. If the daemon is unreachable, start it
  with `"/mnt/c/Program Files/Docker/Docker/Docker Desktop.exe" &` and wait for `docker info` to succeed;
  if that doesn't work, ask the user rather than working around it.

## Finishing work

Before handing work back, run `scripts/test.sh` and make sure the whole suite is green, end-to-end tests
included. If something fails, fix it or report the failure with its output; never describe work as done
while the suite is red or while the e2e tests were skipped. If the gate could not run (for example Docker
is unavailable), say so plainly instead of implying it passed.
