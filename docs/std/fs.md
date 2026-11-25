# std.fs

std.fs: files and folders. `main` asks for the `Disk`. Tests pass a `MemFiles`.

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

Files kept in memory for tests, by path. Folders are implied by the files' paths.

## Traits

### Files

```rust
capability trait Files {
  /// The whole file as UTF-8 text.
  fn read(self, path: String) -> Result[String, FsError]
  /// The whole file as bytes, each an `Int` from 0 to 255.
  fn read_bytes(self, path: String) -> Result[List[Int], FsError]
  /// Replaces the file's contents, creating the file if needed.
  fn write(mut self, path: String, text: String) -> Result[(), FsError]
  /// Adds `text` at the end of the file, creating the file if needed.
  fn append(mut self, path: String, text: String) -> Result[(), FsError]
  /// Whether a file or folder exists at `path`.
  fn exists(self, path: String) -> Bool
  /// The entry names of a folder, sorted.
  fn list(self, path: String) -> Result[List[String], FsError]
  /// Creates a folder and any missing parents.
  fn make_dir(mut self, path: String) -> Result[(), FsError]
  /// Removes a file or an empty folder.
  fn remove(mut self, path: String) -> Result[(), FsError]
  /// Replaces the file's contents with `bytes`, each from 0 to 255, creating the file if needed.
  fn write_bytes(
    mut self,
    path: String,
    bytes: List[Int],
  ) -> Result[(), FsError]
  /// Whether `path` is a folder.
  fn is_dir(self, path: String) -> Bool
  /// Moves a file to `to`, replacing what is there.
  fn rename(mut self, path: String, to: String) -> Result[(), FsError]
  /// Removes a file, or a folder and everything in it.
  fn remove_all(mut self, path: String) -> Result[(), FsError]
  /// The path from the file system's root, with `..` and links resolved. `NotFound` if nothing is there.
  /// A stand-in's paths are already absolute.
  fn absolute(self, path: String) -> Result[String, FsError]
}
```

Reads and writes files: the `Disk`, a `MemFiles`, or a mock.

## Functions

### mem_files

```rust
fn mem_files(files: List[(String, String)]) -> MemFiles
```

An in-memory file system holding `files` as (path, contents) pairs.

## Implementations

- `impl Files for Disk`
- `impl Files for MemFiles`
