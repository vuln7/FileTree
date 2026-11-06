# FileTree

A file browser that is an actual tree. Your disks start as little blossoms, every folder you open grows a branch, and every file is a flower.

It's a full file manager: preview, rename, move, copy, zip, and delete, with undo. It works on your own files and doesn't use the internet.

## Run it

Download [FileTree.exe](FileTree.exe) and double-click it. There's nothing to install. It needs 64-bit Windows 10 or 11.

The exe isn't signed, so Windows may warn you the first time. Choose **More info**, then **Run anyway**.

Deleted files go to the Recycle Bin, nothing is ever overwritten, and changes can be undone.

## Build it yourself

Install [Rust](https://rustup.rs), then:

```bash
cargo run --release
```

## License

[MIT](LICENSE). The fonts in `assets/fonts` are under the SIL Open Font License. Licenses for the libraries inside the exe are in [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
