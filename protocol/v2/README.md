# FlowType protocol v2

V2 uses UTF-8 JSON in WSS text frames. Every message contains `protocol_version`
and `type`. Identifiers are UUID strings and `sequence` is a positive signed
64-bit integer.

The files in this directory are language-neutral contract fixtures. Android and
Rust tests must accept the valid examples and reject the invalid examples.

Core constraints:

- A text snapshot is complete, not an edit operation.
- `start`, `update`, and `finish` carry `session_id`, `sequence`, and `full_text`.
- A retargeting `start` carries `replaces_session_id`. Windows atomically ends
  exactly that old session before capturing the current cursor for the new one.
- A user-triggered full sync at a new cursor sets `attach_existing=true` on
  `start`. Windows checks only the immediately preceding text once: an exact
  full-text match is adopted as the new TSF range without inserting another
  copy. Automatic first input and later updates do not perform this check.
- `cancel` abandons pending synchronization and releases the Windows session without changing the target text.
- ACKs are cumulative and identify the latest successfully applied sequence.
- Reusing one sequence with different text is a protocol error.
- A UTF-8 JSON message is limited to 1 MiB.

Before business messages, Windows sends a fresh `challenge` containing its PC
identifier and nonce. Android signs the domain-separated PC ID, phone ID, and
nonce with its per-PC Keystore P-256 key. A first `pair` also carries the
one-time QR token and public key; later `authenticate` messages carry only the
identity and signature. Windows replies with `ready`. These authentication
envelopes use the same `protocol_version` but are separate from session messages.

After reconnecting an active session, Android sends `resume` with its cumulative
ACK, latest sequence, complete text, and active/finishing state. If a new session
has not been acknowledged, Android retries it as `start` with the persisted
`replaces_session_id`. An offline draft that never locked a Windows target is not
resumed automatically.

Image clipboard transfer is independent of the text session. Android sends one
`image_start` JSON text frame followed immediately by one binary frame. The
header contains `transfer_id`, `phone_id`, `mime_type`, decoded dimensions,
`byte_length`, lowercase SHA-256, and `original`. Windows validates the header,
payload digest, encoded format, and decoded pixel count before replacing the
current user's clipboard, then replies with `image_ack` or `image_error`.

Only JPEG and PNG are accepted. One image may be in flight per connection.
Optimized images are limited to 15 MiB, original images to 32 MiB, and decoded
images to 40 million pixels. Images are not pasted automatically and are not
added to text history.
