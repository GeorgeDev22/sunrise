# Loading and saving chunk attachments

`chunk_data_load` runs on the chunk-decode worker thread and `chunk_data_save` runs on the
persistence worker thread, never with live world access. Both receive `&dyn ChunkDiskHost`, which
supplies only the job identity (`host.job()` -> `&ChunkDiskContext`: world epoch, dimension,
coordinates, full/proto kind and phase) and immutable resource reads. `registry(name)` lends
`Option<&[String]>` in registry ID order from the worker's frozen generation (`None` means that
generation has no such registry; copy it if it must outlive the callback). Neither metadata getter
reads files; `ConfigHost::setting` returns an owned `Option<String>` from the same snapshot. The
host has no live backend and is not a `ServerHost`. Typed values need a persistence codec, not
`Wire`. Unknown bounded attachment entries are preserved opaquely.

For both hooks, codec errors, invalid or over-budget disk NBT, and panics refuse publication of
the job's edits. Later listeners still see the last accepted envelopes; `event.data.failure()`
reports only the first failure of the current callback, never an earlier listener's error. Each
operation runs and reports its own refusal even after an earlier refusal in the same callback.
A phase refusal drops only that callback's proposal and is reported each time without disabling
it; only a panic disables a callback for that hook for the session.

## Loading (`chunk_data_load`)

`chunk_data_load` fires with `ChunkDataLoadEvent` after a full or proto disk record has decoded,
before its attachment store is published. A missing record, fresh generation and proto-to-full
promotion do not fire this hook. Use `event.data.get(&KEY)` and `event.data.set(&KEY, value)` to
read and edit the typed store (for example, reading disk value 8 and setting 9 publishes 9 on
success).

If the job fails, the chunk still loads with the attachments exactly as they were stored. No
`chunk_data_save` listener runs for that chunk for the rest of the session, so half-initialised
state never replaces the stored record, and the chunk's later saves write its stored attachments
back unchanged. The load diagnostic names the chunk and the mod; fix the mod and restart before
retrying a disabled callback.

## Saving (`chunk_data_save`)

`chunk_data_save` fires with `ChunkDataSaveEvent` on the persistence worker after capture and
before encoding. To serialize a mod block entity's live state into its own payload, use
[`block_entity_save_additional`](../../concepts/parts/block-entity-types.md#saved-data) on the
server thread, before the disk worker sees the capture. If block-entity serializer edits cannot
be written, the worker retries with stored block-entity payloads and keeps the result of
`chunk_data_save`.

Use `event.data`'s `get`, `set` and `remove` methods with an `AttachmentType<T>` that has a
persistence codec. An ignored codec error still refuses the whole job; later listeners'
successful edits cannot make that job publish. Capturing 7, writing 8 in the callback and later
writing 9 in the live chunk produces disk value 8 without changing live value 9.

A captured record caches its callback result, so disk encoding and pending reads reuse it and
never invoke the callback twice. If the writer stops before running a job, pending readers can
still read the captured snapshot. Records are handled independently: an encoding refusal leaves
the previous region slot intact while unrelated records still write; a pending reader that
already borrowed the snapshot finishes reading it, and a fresh load after the failed attempt
reads the previous disk record. A chunk whose `chunk_data_load` listener failed is saved without
any save listener.

Every record the writer processes runs the callback exactly once, for full saves (explicit,
forced, late, unload and closing) and proto saves alike. Saves are refused, with no callback,
while an action savepoint is open, and chunk unloading waits for it to close. A record that
cannot be encoded or written, including one whose listener panics, is reported with its
coordinates and storage type. A flush waits for all attempts to finish. An individual record
failure does not fail the command, timer save or shutdown, but a failed region sync fails the
flush. The server does not retry the failed snapshot; a later edit can be saved in a new
snapshot.

## Disk NBT budgets

Both load and save check the uncompressed disk NBT: at most 65,536 bytes per named `value` tag,
64 container levels in the value, and 1,048,576 bytes for the whole `sunrise:attachments` entry.
These budgets include tag IDs, names, compound terminators, mixed-list wrappers, empty namespaces
and opaque unknown data as applicable. Over-budget data or data that would need truncation (such
as an overlong modified UTF-8 string) refuses the job.
