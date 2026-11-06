<p align="center">
  <img src="docs/banner.png" alt="FileTree. Three cut-paper flowers growing from green soil on an ultramarine background." width="100%">
</p>

<h3 align="center">A file browser that is an actual tree.</h3>

<p align="center">
  Your disks start as little blossoms. Every folder you open grows a branch,<br>
  and every file is a flower that blooms.
</p>

<p align="center">
  <a href="https://github.com/vuln7/FileTree---A-file-browser-that-is-an-actual-tree./raw/main/FileTree.exe">
    <img src="https://img.shields.io/badge/Download_for_Windows-FileTree.exe-128a58?style=for-the-badge&labelColor=14205c" alt="Download FileTree.exe for Windows" height="44">
  </a>
</p>

<p align="center">
  <sub><b>Nothing to install.</b> Download it, double-click it, and it opens on your own disks.</sub>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-10_%7C_11_(64--bit)-1f3fb3?style=flat-square" alt="Windows 10 or 11, 64-bit">
  <img src="https://img.shields.io/badge/size-9.7_MB-1f3fb3?style=flat-square" alt="9.7 MB">
  <img src="https://img.shields.io/badge/built_with-Rust-1f3fb3?style=flat-square" alt="Built with Rust">
  <img src="https://img.shields.io/badge/license-MIT-1f3fb3?style=flat-square" alt="MIT license">
</p>

<br>

<p align="center">
  <img src="docs/grow.gif" alt="Clicking a folder grows a branch, and its files bloom into flowers." width="86%">
</p>

> [!NOTE]
> The exe isn't signed yet, so Windows may warn you the first time. Choose **More info**, then **Run anyway**. FileTree doesn't use the internet, and your files stay on your computer.

<br>

## Everything you'd open Explorer for

<table>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/preview.webp" alt="A photo selected on the tree, with its preview and details in the side panel.">
      <br><b>See it without opening it</b>
      <br><sub>Click a flower for a preview, its size and dates, and everything you can do with it.</sub>
    </td>
    <td width="50%" valign="top">
      <img src="docs/quicklook.webp" alt="Quick Look showing a picture across the whole window.">
      <br><b>Quick Look</b>
      <br><sub>Press <kbd>Space</kbd> to fill the window. Zoom, pan, and step through the folder with the arrow keys.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/menu.webp" alt="The right-click menu open on a flower.">
      <br><b>Right-click for everything</b>
      <br><sub>Rename, duplicate, copy, cut, zip, unzip, open with, and open a terminal here.</sub>
    </td>
    <td width="50%" valign="top">
      <img src="docs/code.webp" alt="A source code file previewed with coloured keywords and strings.">
      <br><b>Read code in colour</b>
      <br><sub>Text and source code show right in the panel. So does what's inside a zip.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/audio.webp" alt="An audio player in the panel, partway through a track.">
      <br><b>Play music where it is</b>
      <br><sub>MP3, WAV, FLAC, OGG, and more, with a seek bar. Nothing else needs to open.</sub>
    </td>
    <td width="50%" valign="top">
      <img src="docs/crown.webp" alt="A folder of 2,400 files shown as a wide crown of blossoms.">
      <br><b>Thousands of files become a crown</b>
      <br><sub>Huge folders stay readable and fast. Zoom in and every file is its own flower.</sub>
    </td>
  </tr>
</table>

<br>

## Drag it. Undo it.

<p align="center">
  <img src="docs/drag.gif" alt="A flower carried onto a folder, then put back with one click on Undo." width="86%">
</p>

Carry a flower onto any folder to move it, or hold <kbd>Ctrl</kbd> to copy. Hold over a closed folder and it opens for you.

**Your files are safe:**

- Deleting moves things to the Recycle Bin, and you can browse and restore from it right in the app.
- Nothing is ever overwritten. If a name is taken, the new one becomes `name (2)`.
- Moves, renames, copies, new items, and deletes can all be undone.
- Changing something Windows relies on asks you first.

<br>

## A flower for every kind of file

<p align="center">
  <img src="docs/flowers.webp" alt="Nine flowers: daisy for documents, poppy for images, trillium for video, bellflower for audio, star for code, quince for data, allium for archives, dahlia for apps, and clover for everything else." width="100%">
</p>

Told apart by shape as well as colour, so you can read a branch at a glance.

<br>

## Light enough to forget

It draws directly on your graphics card with no browser engine inside, and stops drawing the moment nothing is moving.

| | Measured on one Windows 11 PC |
|---|---|
| **Window on screen** | About half a second |
| **CPU while still** | Effectively none |
| **Memory while active** | About 155 MB |
| **Memory after a pause** | About 5 MB or less in use, with about 180 MB kept reserved |

It adapts to your machine by itself: your disks, your folders, your screen scaling, and your reduced-motion setting. With no working graphics card, it falls back to drawing in software.

<br>

## Shortcuts

<details>
<summary><b>Show all the shortcuts</b></summary>
<br>

| Action | Mouse | Keyboard |
|---|---|---|
| Open or close a folder | Click its bud | <kbd>Space</kbd> or <kbd>Enter</kbd> |
| Close everything | Sprout button | <kbd>Ctrl</kbd> <kbd>R</kbd> |
| Move around the tree | Click | <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> |
| Open a file | Double-click | <kbd>Enter</kbd> |
| Quick Look | Click the preview | <kbd>Space</kbd> |
| Rename | Pencil button | <kbd>F2</kbd> |
| Copy, cut, paste | Panel buttons | <kbd>Ctrl</kbd> <kbd>C</kbd>, <kbd>Ctrl</kbd> <kbd>X</kbd>, <kbd>Ctrl</kbd> <kbd>V</kbd> |
| Duplicate | Right-click | <kbd>Ctrl</kbd> <kbd>D</kbd> |
| Move to the Recycle Bin | Bin button | <kbd>Del</kbd> |
| New folder | Right-click | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>N</kbd> |
| New file | Right-click | <kbd>Ctrl</kbd> <kbd>N</kbd> |
| Undo | Undo on the message | <kbd>Ctrl</kbd> <kbd>Z</kbd> |
| Select several | <kbd>Ctrl</kbd>-click, or <kbd>Shift</kbd>-click in the list | <kbd>Ctrl</kbd> <kbd>A</kbd> |
| Search or go to a path | Search box | Just start typing |
| Pan and zoom | Drag and scroll | |
| Fit the whole tree | Corner button | <kbd>Home</kbd> |
| Back and forward | Toolbar | <kbd>Alt</kbd> <kbd>←</kbd>, <kbd>Alt</kbd> <kbd>→</kbd> |
| Up a level | Toolbar | <kbd>Backspace</kbd> |
| Show hidden files | Eye button | <kbd>Ctrl</kbd> <kbd>H</kbd> |

</details>

<details>
<summary><b>What it doesn't do yet</b></summary>
<br>

- Video and PDF show a thumbnail. Playing or paging through them opens another program.
- It can't permanently delete anything or empty the Recycle Bin. That's on purpose.
- Permissions, sharing, and network drives aren't covered.
- Only zip archives can be looked inside or unpacked.
- The download is for Windows. The code is written with macOS and Linux in mind, but they're untested.

</details>

<details>
<summary><b>Build it yourself</b></summary>
<br>

Install [Rust](https://rustup.rs), then:

```bash
cargo run --release
```

| File | What it does |
|---|---|
| `src/fs.rs` | Disk access on background threads: listing, watching, searching, previews |
| `src/ops.rs` | Changes to files, and how to undo each one |
| `src/tree.rs`, `src/layout.rs` | The tree, and shaping it so branches never overlap |
| `src/scene.rs`, `src/shaders.wgsl` | Motion, and drawing all the paper with no meshes or textures |
| `src/ui.rs`, `src/overlay.rs` | Toolbar, panel, menus, dialogs, and Quick Look |
| `src/app.rs`, `src/actions.rs` | State, behaviour, and every command |
| `src/capture.rs` | With `--features capture`, runs the app from a script with no window |

Run the tests with `cargo test`.

</details>

<br>

## License

[MIT](LICENSE). The fonts in `assets/fonts` are under the SIL Open Font License. Licenses for the libraries inside the exe are in [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
