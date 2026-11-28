from std.fs use Files, Disk, FsError

/// The text of the bytes `bs`, unchecked: through a file.
fn text_of(mut disk: Disk, bs: List[Int]) -> Result[String, FsError] {
  mut disk.write_bytes("bytes.txt", bs)!
  disk.read("bytes.txt")
}

fn main(mut disk: Disk) -> Result[(), FsError] {
  spaced := text_of(mut disk, [32, 128, 128])!
  print(byte_len(trim(spaced)))
  cut := text_of(mut disk, [97, 98, 240])!
  print(byte_len(f"{cut:.3}"))
  print(to_upper(cut).bytes())
  print(is_alpha(text_of(mut disk, [240])!))
  Ok(())
}
