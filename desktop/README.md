# DataPack Desktop

Internal development build · Windows x86_64 · version 0.1.0

DataPack compresses flat data into `.dpack` archives and restores the original
bytes. All operations run locally. There are no accounts, uploads, or telemetry.

## Launch

Extract the complete internal Windows ZIP into a folder, then double-click
`datapack-desktop.exe`. This is an unsigned internal executable, not a public
installer. The ZIP also contains this guide, license texts, and `demo.csv`, a
small synthetic demonstration file.

From a source checkout with Rust 1.85.0 and the Visual Studio C++ build tools:

```powershell
cargo run --manifest-path desktop/Cargo.toml --release --locked
```

The normal source build appears at
`desktop/target/release/datapack-desktop.exe`, unless `CARGO_TARGET_DIR` is set.
The actual window requires Windows; Linux supports adapter development/tests.

## A short demonstration

1. Choose **Compress data**, then **Browse** to select `demo.csv`.
2. Click **Analyze**. Review detected format, delimiter, sampling scope,
   recommendation, and resource estimate. Partial analysis describes a sample;
   its estimated savings are not a guarantee for the entire file.
3. Keep **Automatic / Recommended**, or choose **Chunked** for bounded in-flight
   data and embedded integrity checks. **Show details** explains the settings.
4. Choose a new destination filename and click **Compress file**. Progress comes
   from the engine. When a phase has no known total, the bar is indeterminate.
5. Inspect archive size, storage reduction, method, and output path. Click
   **Validate or restore result**; the original path is carried over for comparison.
6. Click **Validate archive**. Check the explicit Valid/INVALID state, source
   match, archive version, expected output size, and individual checks in details.
7. Choose a new restored filename and click **Decompress archive**. Inspect the
   output path, restored size, and the reported integrity evidence.

To use an existing archive, choose **Validate & restore** and browse to its
`.dpack` file. The optional original-file field enables source comparison.
Paths and result text can be selected and copied using Ctrl+C. Use Tab to move
between controls and Space to activate a focused button.

## Integrity and cancellation

**Exact bytes** means preserving the original file representation, including
whitespace, quoting, line endings, and formatting. V2 checks restored data against
embedded SHA-256 values. V1 has no embedded SHA-256: supply the original during
validation to establish exact source agreement. Creating an archive by itself
is not a verification claim.

**Cancel operation** requests a cooperative stop at the next safe engine
checkpoint. Some structured phases may take longer to stop. Uncommitted partial
output is cleaned up; existing destinations remain protected. If the engine
already committed the result, completion wins over a late cancellation request.
You can retry after cancellation or failure. Closing during work offers safe
cancellation and waits for the worker before closing.

DataPack never enables overwrite. If a destination exists, choose another
filename. The save dialog never grants overwrite permission. Keep selected source/archive files unchanged during an
operation. Exact comparisons describe the files read during that operation.

## Foundation limitations

Windows native controls and system fonts are used; there is no custom dark theme
or official application icon yet. A Windows default developer icon is temporary.
No recent-file list, saved preferences, installer, file association, or automatic
update is installed. The native window smoke checks launch and controls, not
stakeholder visual acceptance. Operator review on Windows remains required.
