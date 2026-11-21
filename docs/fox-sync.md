# Fox game synchronization

Games → Sync settings → Fox sync accepts a Fox username. Public records need no
password or Keychain entry. Katastro checks every 15 minutes while open, beginning
at startup, and offers a manual sync and an automatic-sync toggle. The default
scope is the latest 200 games; changing the limit never removes earlier downloads.

The shared Games table offers a filter for each server, a sortable
Server column, search, ranks beside names, and trophies based on each archive's
own player name. Sync controls name their server. Training setup selects one
server/account. Idle preparation processes the connected archives sequentially on the
existing engine. Opening a game, pausing, or entering training still takes priority.

## Protocol and references

This is an independent implementation of Fox's public web archive, checked against
live responses on 2026-11-15. These endpoints are undocumented and may change:

- `https://newframe.foxwq.com/cgi/QueryUserInfoPanel` resolves `username`, with
  `srcuid=0`, to the numeric `uid` and canonical username.
- `https://h5.foxwq.com/yehuDiamond/chessbook_local/YHWQFetchChessList` takes
  `dstuid`, `type=1`, and **`lastCode`**. Start with `0`, then use the complete last
  record's `chessid`, retained as a string. The lowercase `lastcode` is ignored.
  The response's numeric cursor is not used. Pages normally contain 100 entries,
  occasionally more, and provide no total. A shorter page ends traversal.
- `https://h5.foxwq.com/yehuDiamond/chessbook_local/YHWQFetchChess` takes `chessid`
  and returns the original SGF in the JSON `chess` field.

Research began with [WeiqiHub](https://github.com/ale64bit/WeiqiHub), revision
`cd1997863d7e2e6d6105c4b6f0580f607170ba19`. Its
[submodule declarations](https://github.com/ale64bit/WeiqiHub/blob/main/.gitmodules)
point to a Fox implementation that was not publicly accessible. Its public build
has OGS; the full Fox-enabled build requires the additional component.
[MiniFox's FAQ](https://walruswq.com/minifox) explicitly describes MiniFox as closed
source. The author's [separate API](https://github.com/openfoxwq/api) requires
application credentials and is not used. The public
[foxwq-sgf-dl](https://github.com/yiqiaoli/foxwq-sgf-dl) project helped confirm
related endpoint names and cursor conventions; the unauthenticated web endpoints
above were verified directly. No reference implementation code was copied.

## Storage and failure policy

`archive` shares the worker, cancellation, SQLite index, immutable-file storage,
recent-history cap, and retry policy across providers. Protocol implementations
are separate; `pandanet` remains an alias for the original public application API.
Each provider has a separate directory and database under Application Support /
Katastro (`Pandanet`, `Fox`, and `OGS`). Existing Pandanet storage and Keychain entries
need no migration. Fox originals retain their exact downloaded bytes, including
Fox komi encoding; the existing SGF importer normalizes analysis semantics.
Ranks and results in the table come from the parsed SGF. Chinese rank suffixes
are displayed as `k`/`d`.

The account and server record ID identify a download. Existing indexed files are
skipped without rehashing; missing files are restored when their page is scanned.
Initial sync, changed scope, outstanding failures, and daily sweeps scan the chosen
range. Other checks stop at a fully known page. Two transfers run concurrently.
Validated originals are published atomically without overwriting a different
local file. Individual failures are retained for retry; HTTP 429/503 stop the
cycle. Canceling stops subsequent work; an in-flight request has a 20-second bound.

Fox requests use HTTPS to its two official hosts, disable redirects, and limit
responses to 16 MiB. Loopback HTTP endpoints are injectable for portable tests.
Private archives, foreign account/game IDs, mismatched SGF response IDs, repeated
pages, malformed responses, and unparseable SGFs produce errors without saving
those records. No third-party service receives an account password. Private
records are unsupported by this public-archive integration.

## Verification

Portable integration checks use synthetic HTTP responses, real SQLite databases,
real SGF imports, application commands, real egui input, and a controlled engine
subprocess:

```sh
cargo test --test fox_sync --test fox_ui --test pandanet \
  --test pandanet_background --test pandanet_ui --test training_background
```

The opt-in live check requires network access and an authorized public Fox account.
It defaults to temporary storage. Set `KATASTRO_FOX_ROOT` only to keep imported
originals and the automatic-sync configuration at that location:

```sh
KATASTRO_FOX_USERNAME='<your username>' cargo test --test fox_sync \
  live_fox_archive_imports_and_reuses_public_games -- --ignored --nocapture
```

The live Rust check imported and parsed all 57 records available for the authorized
account; an immediate repeat downloaded zero games. A separate live pagination
probe retrieved nonoverlapping older records for an archive with over 100 games.
Personal records, account details, captures, screenshots, and test logs remain
outside Git. The library GPU preview uses synthetic records:

```sh
KATASTRO_FOX_UI_PREVIEW=local/fox-library.png cargo test --test fox_ui \
  server_filter_opens_fox_original_and_training_uses_fox_account
```

Initial behavioral failures covered passwordless connection, absent server filters,
wrong training preparation for the second account, ambiguous sync controls, and
review indexing under an opponent’s name instead of the archive owner. Source
paths are canonicalized on the worker when libraries change, matching review
source identity without filesystem lookups on every analysis response.
Nine temporary mutations verify the recent-game cap, cache reuse, missing-file
recovery, SGF result metadata, server filtering, training account selection,
preparation of both libraries, archive-owner priority when an opponent shares a
configured username, and SGF response identity. All must fail their
relevant integration tests; production behavior is restored afterward.
