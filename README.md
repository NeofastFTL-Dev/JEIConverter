# JEIConverter

Converts a **JEIExporter** dump (Minecraft 1.12.2) into the `recipes.json` file read by
**[MCRPV](https://github.com/NeofastFTL-Dev/MCRPV)** (Minecraft Recipe Tree Viewer). It is the
missing half of that app: the tree viewer knows how to draw a recipe graph, this tool builds one
from what JEI already knows.

It is a single-file console program with no dependencies beyond the .NET base class library.

## How it works

```
exports/
├── nameMap.json          ──► registry id → pretty name ("minecraft:iron_ingot" → "iron_ingot")
└── recipes/
    ├── crafting_shaped.json
    ├── furnace.json
    └── …                 ──► one pass per file, in priority order
                                   │
                     output / result / outputs / product field
                     (or the slot whose role is output)
                                   │
                     inputs: slots, inputs, ingredients, catalysts
                                   │
                     duplicates merged, counts summed
                                   │
                     first recipe for an item wins  ──►  recipes.json
```

Files are processed in a fixed order so the clean, well-structured categories win over exotic ones:
crafting/workbench/shaped/shapeless first, then furnace/smelting, then alloy/infuser/crusher, then
everything else. When two sources describe the same output item, the first one parsed is kept.

## What it understands

The converter is deliberately forgiving — JEIExporter dumps differ between versions and mods — but it
only reads fields it recognises:

| Part | Accepted fields |
|---|---|
| Output | `output`, `result`, `outputs`, `out`, `product`, or a `slots` entry with `role`/`type`/`kind` containing `output`, `result` or `product` |
| Item id | `item`, `id`, `name`, `registry_name`, `registryName`, `ingredient`, `stack`, a `stacks` array (first entry), `value`, `content`, or a bare string |
| Count | `count`, `amount`, `Count` (defaults to 1) |
| Inputs | `slots` (non-output), `inputs`, `ingredients`, `input`, `catalysts` |

Item ids are cleaned before they are written: NBT suffixes (`{…}`) and a leading `item:` are removed,
a `mod:` namespace is dropped unless `nameMap.json` knows the full id, and the result is lowercased
to `[a-z0-9_]`. Duplicate ingredients inside one recipe are merged and their counts summed.

## Usage

From the repository root:

```bash
dotnet run --project JEIConverter/JEIConverter -- "<dump folder>" "<output recipes.json>"
```

Both arguments are optional:

* no input folder → the program asks for it;
* no output path → it writes `recipes.json` into the current directory.

Example:

```bash
dotnet run --project JEIConverter/JEIConverter -- ~/Downloads/exports ./recipes.json
```

Every processed file is printed with the number of recipes it contributed, and the run ends with the
total number of unique items. A file that cannot be parsed is skipped with a message instead of
aborting the run.

Where the input lives: JEIExporter normally writes `exports/` (sometimes `jei_exports/`) into the
instance folder, with `nameMap.json` next to a `recipes/` subfolder. The converter prefers
`exports/recipes/` when it exists and otherwise scans the folder recursively, ignoring `nameMap.json`
itself. `nameMap.json`, `namemap.json`, `itemMap.json` and `recipes/nameMap.json` are all accepted as
the name map.

## Limits

* **Best effort, by design.** Crafting and smelting convert well; machine categories (NuclearCraft,
  EnderIO, Extended Crafting, Astral Sorcery, …) are often incomplete or missing, because their dumps
  do not follow the simple item-in/item-out shape.
* **Output counts are ignored.** Only the ingredient list is exported; producing 4 planks per log is
  not represented, since the tree format scales ingredients, not yields.
* **Alternative ingredients collapse to one.** A slot that accepts any of several items becomes the
  first stack found — there is no tag/ore-dictionary support in the tree app.
* After converting, check the output by hand. Recipes that matter to you are worth adding manually;
  the format is small enough to edit.

## Building on this machine

The project targets plain `net8.0` (no Windows-specific APIs), so it builds and runs natively on
Linux with the SDK already installed in `~/.dotnet`.

```bash
# from the repository root
dotnet build JEIConverter/JEIConverter.csproj -c Release
dotnet run --project JEIConverter/JEIConverter -- "<dump folder>" "<output recipes.json>"
```

`~/.dotnet/dotnet --list-sdks` on this machine reports **8.0.428** and **10.0.404**.

### Windows executable

The repository root carries the script for the whole sequence:

```bash
./build-wine.sh              # -> JEIConverter/dist/wine-x64/JEIConverter.exe
./build-wine.sh --dry-run    # print the three commands instead
```

Run the result with `./run-wine.sh "<dump folder>" "<output recipes.json>"`, or directly with
`wine JEIConverter/dist/wine-x64/JEIConverter.exe …`.

By hand, from `JEIConverter/JEIConverter` (the folder with the `.csproj`):

```bash
WINEPREFIX=~/.wine-dotnet wine 'C:\dotnet\dotnet.exe' publish JEIConverter.csproj \
  -c Release -r win-x64 --self-contained true --no-restore \
  -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true \
  -p:DebugType=none -o dist/wine-x64
```

(restore first with `~/.dotnet/dotnet restore JEIConverter.csproj` if the packages are not cached —
the Wine-side NuGet cannot complete HTTPS requests here). The result is
`dist/wine-x64/JEIConverter.exe`, about 68 MB self-contained. It is a console program: run it from
`cmd.exe` or Windows Terminal, not by double-clicking, so you can see the prompts and summary.

## Screenshots

This is a console tool, so the "screenshots" are terminal captures. They belong in
`docs/screenshots/` and none are committed yet:

| File to add | What to capture |
|---|---|
| `docs/screenshots/console-run.png` | a full run: input folder, the per-file `+N recipes` lines, the final total |
| `docs/screenshots/output.png` | the generated `recipes.json` open in an editor, a few entries visible |

```html
<!-- Uncomment once the files exist:
![Console run](docs/screenshots/console-run.png)
![Generated recipes.json](docs/screenshots/output.png)
-->
```

## Troubleshooting

* **“Fout: map bestaat niet.”** — the path passed as the first argument does not exist; quote paths
  that contain spaces.
* **Almost no recipes converted** — the dump is probably not under `exports/recipes/`, or its files
  use field names the converter does not know. Check the per-file `+N` counts to see which file
  contributed nothing.
* **Item names look like `iron_ingot`** — that is expected: the tree app matches on plain names, and
  `nameMap.json` only replaces them when it has an entry.

(The program's own messages are in Dutch, matching the rest of the project's UI; this README and the
output file format are English.)

## Credits

* JSON and regex handling: **System.Text.Json** and **System.Text.RegularExpressions** from .NET (MIT).
* Dumps are produced by **JEIExporter** for **Just Enough Items** (JEI) on Minecraft 1.12.2; recipe
  data itself belongs to Project Ozone 3 and the individual mod authors.
* Output format designed for MCRPV.

## License

MIT - see [LICENSE](LICENSE).
