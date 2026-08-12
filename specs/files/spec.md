---
aliases:
  - mcp-execution-files spec
  - Virtual Filesystem spec
tags:
  - sdd
  - spec
  - files
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../codegen/spec]]"
  - "[[../server/spec]]"
---

# Block: Virtual Filesystem & Export (`mcp-execution-files`)

> [!abstract]
> Path: `crates/mcp-files`. An in-memory, read-only VFS for generated tool
> files, plus a high-performance, **atomic** export to the real filesystem.
> Depends on `mcp-execution-codegen` (for `GeneratedCode` and its derived
> resource bounds) and, since #504, `mcp-execution-core` directly (for
> `confinement::open_confined_write`).

## 1. Responsibility

Stage `mcp-codegen`'s `GeneratedCode` (or any hand-built file set) entirely
in memory as a `FileSystem`, validate every path, then publish it to disk
such that:
- a process killed mid-export never leaves a half-written tree at the
  target path, and
- a failed export never corrupts a previously-published one.

## 2. Public API Surface

```rust
// crate root
pub use builder::FilesBuilder;
pub use filesystem::{ExportOptions, FileSystem};
pub use types::{FileEntry, FilePath, FilesError, FilesResourceKind, Result};

pub struct FilePath(String); // validated: absolute ('/'-prefixed), no "..", no empty/"." components, Unix-style on all platforms
impl FilePath {
    pub fn new(path: impl AsRef<Path>) -> Result<Self>;
    pub fn as_path(&self) -> &Path;
    pub fn as_str(&self) -> &str;
    pub fn parent(&self) -> Option<Self>;
}

pub struct FileEntry { /* content: String */ }
impl FileEntry {
    pub fn new(content: impl Into<String>) -> Self;
    pub fn content(&self) -> &str;
    pub fn size(&self) -> usize;
}

pub struct FileSystem { /* files: HashMap<FilePath, FileEntry> */ }
impl FileSystem {
    pub fn new() -> Self;
    pub fn add_file(&mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Result<()>;
    pub fn read_file(&self, path: impl AsRef<Path>) -> Result<&str>;
    pub fn exists(&self, path: impl AsRef<Path>) -> bool;
    pub fn list_dir(&self, path: impl AsRef<Path>) -> Result<Vec<FilePath>>;
    pub fn file_count(&self) -> usize;
    pub fn all_paths(&self) -> Vec<&FilePath>; // sorted
    pub fn files(&self) -> impl Iterator<Item = (&FilePath, &FileEntry)>;
    pub fn clear(&mut self);
    pub fn export_to_filesystem(&self, base_path: impl AsRef<Path>) -> Result<()>;
    pub fn export_to_filesystem_with_options(&self, base_path: impl AsRef<Path>, options: &ExportOptions) -> Result<()>;
    #[cfg(feature = "parallel")]
    pub fn export_to_filesystem_parallel(&self, base_path: impl AsRef<Path>) -> Result<()>;
}

pub struct ExportOptions { pub atomic: bool, confine_to: Option<PathBuf> } // atomic default true; confine_to default None
impl ExportOptions {
    pub const fn new() -> Self;
    pub const fn with_atomic_writes(mut self, atomic: bool) -> Self;
    pub fn with_confine_to(mut self, base_dir: impl Into<PathBuf>) -> Self;
}

pub struct FilesBuilder { /* vfs: FileSystem, errors: Vec<FilesError> */ }
impl FilesBuilder {
    pub fn new() -> Self;
    pub fn from_generated_code(code: GeneratedCode, base_path: impl AsRef<Path>) -> Self;
    pub fn add_file(self, path: impl AsRef<Path>, content: impl Into<String>) -> Self;
    pub fn add_files<P,C>(self, files: impl IntoIterator<Item=(P,C)>) -> Self;
    pub fn build(self) -> Result<FileSystem>;
    pub fn build_and_export(self, base_path: impl AsRef<Path>) -> Result<FileSystem>;
    pub fn file_count(&self) -> usize;
}

pub const MAX_EXPORT_FILES: usize; // = mcp_execution_codegen::progressive::generator::MAX_GENERATED_FILES
pub const MAX_EXPORT_BYTES: usize; // = mcp_execution_codegen::progressive::generator::MAX_GENERATED_BYTES
```

## 3. Input Contract

`FilesBuilder::from_generated_code(code, base_path)` prefixes every
`GeneratedFile.path` with `base_path` (string concatenation, always
forward-slash, so VFS paths stay Unix-style even on Windows) and adds each
as a VFS file. `base_path` is typically `/mcp-tools/servers/<server-id>` or,
for `mcp-server`'s `save_categorized_tools`, the VFS root `/` (with the real
disk target supplied later to `export_to_filesystem`).

## 4. `FilePath` Validation Rules

Rejects (as `FilesError::PathNotAbsolute` or `InvalidPathComponent`):
not starting with `/`; containing `..`; an empty component (doubled `//` or
trailing `/`); a bare `.` component. The **root path `/` itself** is valid
(no components to check). This validation exists specifically to close a
real bug (issue referenced in tests): an empty top-level component from
`"//x.ts"` used to make `base.join("")` resolve to `base` itself, letting a
single-file "group" swap the shared base directory wholesale in
`FilesBuilder::build_and_export`.

## 5. Atomic Export (`export_to_filesystem_with_options`)

1. `check_export_bounds()` — file count ≤ `MAX_EXPORT_FILES`, total content
   bytes ≤ `MAX_EXPORT_BYTES` (CWE-400).
2. `stage_export(target, confine_to)`:
   - Confirms `target`'s parent directory exists.
   - If `options.confine_to` is set (via `ExportOptions::with_confine_to`),
     canonicalizes both the parent and `confine_to` and rejects the export
     with `FilesError::PathEscapesBase` unless the canonicalized parent
     starts with the canonicalized `confine_to` — see
     [[#Confinement check (`with_confine_to`)]]. A canonicalization failure
     on either path (missing directory, permission denied) is a plain
     `FilesError::IoError`, not `PathEscapesBase`, since the confinement
     check itself never ran.
   - Sweeps stale sibling artifacts from a **previous crashed** export of
     the *same* target (`sweep_stale_artifacts`, age-gated — see
     [[#Stale-artifact sweep]]).
   - Creates a fresh sibling temp directory via `tempfile::Builder`
     (prefix `.{target-stem}.staging-...`), canonicalizes it.
   - Pre-creates every directory the file set needs, in one pass.
3. Writes every file into the staging directory
   (`write_file_atomic` per file: temp-file → `fsync` → rename, when
   `options.atomic`).
4. `publish_staged_export`: renames the staging directory into `target`.
   - If `target` doesn't exist yet: a single `rename`.
   - If it does: `target` is moved aside to a unique `.{stem}.stale-{pid}-{nanos}-{seq}`
     sibling first (a directory rename can't replace a non-empty
     destination on any supported platform), the staged directory is
     renamed into `target`, then the displaced original is removed.
     If the *second* rename fails, the displaced original is renamed
     **back** into `target` — so a caller of `swap_into_place` never
     observes `target` missing, except across the narrow window between
     the two renames if the process is killed there (see
     [[#Non-goals / accepted gaps]]).
5. On any failure before step 4 completes, the staging `TempDir`'s own
   `Drop` removes the partial tree — `target` is never touched.

This gives **per-call atomicity for a single `export_to_filesystem` call**,
and (via `FilesBuilder::build_and_export`, see below)
**per-top-level-group atomicity** for a multi-group batch — not
whole-batch atomicity across groups.

### Stale-artifact sweep

`sweep_stale_artifacts` removes orphaned `.{stem}.staging-*`/`.{stem}.stale-*`
siblings left by a **previous process that was killed** before its own
`Drop`/rollback ran. Scoped to the exact target's name (`target_stem`), so
it never touches a concurrent sibling export's own in-flight artifacts.
Gated by `STALE_ARTIFACT_MIN_AGE` = 5 minutes (by mtime): a genuinely
concurrent sibling's staging/displaced directories are always younger than
this within a single export call's real-world duration, so the sweep can
only ever reclaim true crash leftovers, never a live export's artifacts —
this closes a real data-loss race (issue referenced in tests) where a
name-only match could delete a concurrent in-flight export's staging *or*
displaced-backup directory, defeating rollback and permanently losing the
target.

### Confinement check (`with_confine_to`)

`ExportOptions::with_confine_to(base_dir)` is opt-in, defense-in-depth
against a caller-built `target` that was assembled by joining untrusted
input (e.g. a server id) onto a base directory: `PathBuf::join` silently
discards `base_dir` entirely if the joined component is absolute, and a
`..`-bearing component can walk back out of it even for a relative join.
Only `export_to_filesystem_with_options` accepts `ExportOptions`, so this
check is unavailable through `export_to_filesystem` (which always uses
`ExportOptions::default()`, `confine_to: None`) or
`export_to_filesystem_parallel` (which passes `None` directly to
`stage_export`). `mcp-cli`'s `generate` command wires this in as a second
layer behind its primary guard (sanitizing the server-id-derived directory
name) — see [[../cli/spec#generate]].

### Non-goals / accepted gaps

- Concurrent exports of the **same** `base_path` from different processes
  are not locked against each other; the last writer wins (a "lost
  update"), though the sweep guarantees neither loses its own staging/
  rollback artifacts to the other. `mcp-server` mitigates this at a higher
  layer with a per-output-directory `Mutex` — see
  [[../server/spec#Per-resource locking]].
- A process killed **between** `swap_into_place`'s two renames leaves
  `target` transiently absent, with the previous export sitting at a
  `.stale-*` sibling until a later export's sweep reclaims it — accepted as
  a louder failure mode (a visibly missing directory) than the silent
  broken-import bug this design replaces.

## 6. `FilesBuilder::build_and_export`

Unlike `FileSystem::export_to_filesystem` (which treats `base_path` as
exclusively owned, replacing it wholesale), `build_and_export` treats
`base_path` as a **shared** directory (e.g. `~/.claude/servers/`) that other
independent batches (other servers) already occupy:

1. Whole-batch bound check (`vfs.check_export_bounds()`) up front — a
   payload crafted as many small per-group files could otherwise bypass the
   per-group check each `export_to_filesystem` call performs on its own.
2. Splits the VFS by top-level path component into a `BTreeMap<String,
   FileSystem>` of groups (deterministic alphabetical publish order) plus a
   list of bare top-level files (no subdirectory).
3. Each **group** (a real subtree, e.g. `github/createIssue.ts`) is
   published via `FileSystem::export_to_filesystem` into
   `base_path/<group-name>` — inheriting that method's atomic
   staging/swap, and therefore its **replace-not-merge semantics for that
   one group**: re-exporting `github/` with a smaller tool set deletes any
   file previously under `base_path/github` absent from the new batch.
   Sibling groups are unaffected.
4. Each **bare top-level file** (e.g. `/manifest.json`) is written directly
   via its own atomic temp-file-then-rename — always additive/overwriting,
   never deleting.

This gives **per-top-level-group atomicity, not whole-batch atomicity**: if
one group's publish fails partway through a multi-group batch, groups
already published (or bare files already written) remain in place, and the
failure is surfaced to the caller — the batch as a whole is not rolled
back.

## 7. Error Conditions (`FilesError`)

```rust
pub enum FilesError {
    FileNotFound { path },
    NotADirectory { path },
    InvalidPath { path },
    PathNotAbsolute { path },
    InvalidPathComponent { path },
    IoError { path, source: std::io::Error },
    ResourceLimitExceeded { resource: FilesResourceKind, actual, limit }, // is_resource_limit_exceeded()
    PathEscapesBase { path, base },
}

pub enum FilesResourceKind {
    ExportFileCount,
    ExportTotalSize,
}
```
`FilesResourceKind` (`types::FilesResourceKind`, re-exported at crate root) closes what was a
free-form `resource: String` (issue #343), mirroring `mcp-core::ResourceKind`'s closed-enum
pattern (issue #317, [[../core/spec#`Error` / `Result<T>` (`src/error.rs`)]]) without adding
variants to that enum: `mcp-core`'s `ResourceKind` already has semantically adjacent variants
(`GeneratedOutputSize`/`GeneratedFileCount`, the closest neighbors to
`ExportTotalSize`/`ExportFileCount`), but sharing that enum would mean growing it with a variant
pair used by exactly one downstream crate for a single error case — a local enum avoids that
coupling regardless of the dependency edge below. At the time of #343 this crate also had no
direct dependency on `mcp-execution-core` (only a transitive one via `mcp-execution-codegen`),
which was the deciding factor then; #504 has since added a direct dependency for
`confinement::open_confined_write`, so that specific rationale no longer applies, but the
decision to keep `FilesResourceKind` local stands on the enum-scope argument above and was not
revisited by #504. Each variant's `Display` reproduces the same wording `check_export_bounds`
used to build by hand (`"export file count"` / `"export total size"`), so
`FilesError::ResourceLimitExceeded`'s message is unchanged in substance.

`PathEscapesBase` is returned only by `export_to_filesystem_with_options` when
`ExportOptions::with_confine_to` is set and the confinement check fails (see
[[#Confinement check (`with_confine_to`)]]); `path` is the canonicalized
export-target parent, `base` the canonicalized confinement directory. A
canonicalization failure on either side is `FilesError::IoError` instead —
`PathEscapesBase` specifically means "both paths resolved, and the target
is outside the base" (#311).

## 8. Cross-Crate Contracts

- **Consumes** `mcp-codegen::GeneratedCode`/`GeneratedFile`; derives
  `MAX_EXPORT_FILES`/`MAX_EXPORT_BYTES` **equal to**
  `mcp-codegen`'s `MAX_GENERATED_FILES`/`MAX_GENERATED_BYTES` (not
  independently chosen) so a `FileSystem` built from that crate's normal
  output can never be rejected here for merely being "as large as codegen
  already allows."
- **Used by** `mcp-server::save_categorized_tools` (via
  `FilesBuilder::from_generated_code(code, "/").build()` then
  `vfs.export_to_filesystem(&output_dir)` inside `spawn_blocking`, guarded
  by a per-`output_dir` lock — see [[../server/spec#save_categorized_tools]]).
- **Used by** `mcp-cli generate` (via
  `FilesBuilder::from_generated_code(code, "/").build()` then
  `vfs.export_to_filesystem_with_options(output_path, &ExportOptions::new().with_confine_to(base_dir))`
  — one server's tree per call, not the shared-root `build_and_export` path,
  with the confinement check as a second defense-in-depth layer behind
  sanitizing the server-id-derived directory name — see
  [[../cli/spec#generate]]).

## 9. Edge Cases & Notable Behaviors

- Displacing an existing **file** (not directory) at the group-collision
  boundary (e.g. `base/sub` already exists as a plain file, but this batch
  wants `/sub/nested.ts`) is handled: `remove_artifact_best_effort` falls
  back to `fs::remove_file` when `remove_dir_all` fails with
  `NotADirectory`, preventing a permanently-unremovable `.stale-*` artifact
  (a real bug found in review, per test comments).
  `touch_dir` similarly handles both directory (marker-file trick) and
  plain-file (`File::set_modified`) mtime refresh, since the thing being
  displaced isn't always a directory.
- `export_to_filesystem`'s documented performance target is **<50ms for a
  30-file export** (typical GitHub-server case).
- `#[cfg(feature = "parallel")]` `export_to_filesystem_parallel` shares the
  exact same staging/atomic-rename mechanism, writing files via `rayon`
  instead — faster for >50 files, may not preserve write order (order
  doesn't matter for a flat file tree).
- `vfs_to_disk_path`'s defense-in-depth `..`-traversal check (a safety net
  behind `FilePath::new`'s own validation) used to `assert!` on a match,
  panicking the whole process if a `..` ever reached it. It now returns
  `Result<PathBuf, FilesError>`, surfacing `FilesError::InvalidPathComponent`
  instead. All three call sites — `collect_directories`, `write_files`, and
  `export_to_filesystem_parallel`'s own inline call inside its `rayon`
  closure — propagate this via `?` (#318), so a bug that would have reached
  this check is now a normal `Err`, not a crash.

## 10. See Also

- [[../codegen/spec]] — source of `GeneratedCode` and derived resource bounds
- [[../server/spec#Per-resource locking]] — higher-layer serialization for concurrent exports to the same target
- [[../cli/spec#generate]] — CLI-side consumer, via `export_to_filesystem_with_options` with `ExportOptions::with_confine_to`
