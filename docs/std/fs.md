# std.fs

Files and folders.

The file system is a capability: a program gets the real disk only by
asking for it in `main` (`fn main(disk: Disk)`), and passes it on where
`Files` is wanted. Tests pass a `MemFiles` instead.

## Types

### FsError

```rust
type FsError =
  | NotFound { path: String }
  | PermissionDenied { path: String }
  | AlreadyExists { path: String }
  | NotADirectory { path: String }
  | IsADirectory { path: String }
  | OtherFsError { path: String, message: String }
```

Why a file operation failed.

### Disk

```rust
capability Disk {}
```

The real file system. Only the runtime makes one, for `main`.

### MemFiles

```rust
type MemFiles = { files: Dict[String, String] }
```

Files kept in memory, for tests: each path maps to its contents. Folders
are implied by the paths of the files in them.

## Traits

### Files

```rust
capability trait Files {
  /// The whole file as text (UTF-8).
  fn read(self, path: String) -> Result[String, FsError]
  /// The whole file as bytes, each an `Int` from 0 to 255.
  fn read_bytes(self, path: String) -> Result[List[Int], FsError]
  /// Replace the file's contents with `text`, creating the file if needed.
  fn write(mut self, path: String, text: String) -> Result[(), FsError]
  /// Add `text` at the end of the file, creating the file if needed.
  fn append(mut self, path: String, text: String) -> Result[(), FsError]
  /// Whether a file or folder exists at `path`.
  fn exists(self, path: String) -> Bool
  /// The names of the entries of a folder, sorted.
  fn list(self, path: String) -> Result[List[String], FsError]
  /// Create a folder and any missing parents.
  fn make_dir(mut self, path: String) -> Result[(), FsError]
  /// Remove a file, or an empty folder.
  fn remove(mut self, path: String) -> Result[(), FsError]
  /// Replace the file's contents with `bytes` (each an `Int` from 0 to 255),
  /// creating the file if needed.
  fn write_bytes(
    mut self,
    path: String,
    bytes: List[Int],
  ) -> Result[(), FsError]
  /// Whether `path` is a folder.
  fn is_dir(self, path: String) -> Bool
  /// Move a file to `to` (replacing what is there).
  fn rename(mut self, path: String, to: String) -> Result[(), FsError]
  /// Remove a file, or a folder and everything in it.
  fn remove_all(mut self, path: String) -> Result[(), FsError]
  /// The path from the root of the file system, `..` and links resolved;
  /// `NotFound` if nothing is there (a stand-in's paths are already whole).
  fn absolute(self, path: String) -> Result[String, FsError]
}
```

Reading and writing files: the disk, or a stand-in for it.

## Functions

### mem_files

```rust
fn mem_files(files: List[(String, String)]) -> MemFiles
```

An in-memory file system holding `files` (path, contents).

## Implementations

- `impl Files for Disk`
- `impl Files for MemFiles`
