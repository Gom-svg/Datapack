# Validation JSON V1

`datapack validate ARCHIVE --json` emits one UTF-8 JSON document followed by a
newline. `--pretty` changes whitespace only and requires `--json`.

This document defines `ValidationReportV1`. The contract is independent of
the Rust storage structs and frozen archive metadata. Fields listed here are
part of schema version `1`; new incompatible meanings or removals require a
new schema version.

## Invocation and exit status

```text
datapack validate archive.dpack --json
datapack validate archive.dpack --against original.bin --json
datapack validate archive.dpack --against original.bin --json --pretty
datapack validate archive.dpack --max-output-mb 8192 --max-chunks 10000 --max-memory-mb 512 --json
```

`--max-output-mb` and `--max-chunks` are optional. `--max-memory-mb` defaults
to 512 MiB. These controls affect whether validation can safely complete; they
do not change the JSON schema. A nonzero value is required when any of the
three flags is supplied.

- Exit `0`: the report has `valid: true`.
- Exit `1`: archive validation failed, `--against` did not match, or an
  operational/preflight error occurred. When validation produced an archive
  result, the emitted report has `valid: false` and at least one diagnostic;
  invalid limit values and operational errors use stderr with empty stdout.
- Exit `2`: Clap rejected the command line, including `--pretty` without
  `--json`.

An operational error that prevents validation from starting or completing,
such as an unreadable or non-regular input path, uses the normal CLI error
channel rather than fabricating an archive result.

## Root object

| Field | JSON type | Meaning |
|---|---|---|
| `schema_version` | integer | Always `1`. |
| `report_type` | string | Always `"validation"`. |
| `valid` | boolean | True only when every applicable archive check and requested source comparison passed. |
| `archive` | object | Version and size facts, with null fields where a failure prevented safe identification. |
| `checks` | object | Applicability and outcome of each validation level. |
| `against` | object | Result of the optional complete-source comparison. |
| `diagnostics` | array | Stable failure codes and explanatory messages in discovery order. |

The report does not contain file paths, raw field values, reconstructed bytes,
sample rows, or hashes of user data.

## `archive`

`archive` is always present. When a failure occurs before DataPack can safely
identify a value, that field is null.

| Field | JSON type | Meaning |
|---|---|---|
| `version` | integer or null | Header version when readable. Supported values are `1` and `2`; another value accompanies `UNSUPPORTED_ARCHIVE_VERSION`. |
| `format` | string or null | `"dpack_v1"` or `"dpack_v2"` after dispatch to a supported reader. |
| `archive_size_bytes` | integer | Complete archive file size. |
| `original_size_bytes` | integer or null | Restored size declared by validated format metadata. |
| `payload_mode` | string or null | Explicit V1 conversion of the stored payload/archive mode. |
| `chunk_count` | integer or null | V2 chunk count; null for v1. |

`payload_mode` values are:

- `raw_zstd` for v1 `RawZstd`, `Plain`, or `Dictionary` payloads decoded by
  the frozen raw-byte compatibility path;
- `csv_columnar_dictionary` for the frozen v1 DCSV01 payload kind; and
- `chunked_raw_zstd` for v2.

These names describe stored execution semantics. They do not infer the
original file's extension or delimiter.

## `checks`

The checks object has these fields:

| Field | Meaning |
|---|---|
| `header` | Magic, version, fixed header fields, and supported archive mode. |
| `metadata` | Version-defined metadata fields: v1 bincode metadata and outer/inner agreement, or v2 declared size/hash/chunk metadata. |
| `payload_structure` | V1 payload/DCSV01 structure or v2 chunk-table and payload-range structure. |
| `decompression` | Successful zstd and, where applicable, DCSV01 reconstruction. |
| `restored_length` | Complete reconstructed byte count equals the declared original size. |
| `chunk_table` | V2 chunk IDs, sizes, offsets, modes, ordered coverage, and exact payload ranges. |
| `per_chunk_sha256` | All stored per-chunk hashes match. Available only for v2. |
| `global_sha256` | Stored global original-data hash matches. Available only for v2. |
| `trailing_data` | The format defines and the reader confirms the exact archive end. Available for v2; unavailable for v1. |

Each value is one of:

| Value | Meaning |
|---|---|
| `passed` | Applicable check completed successfully. |
| `failed` | Applicable check found an invalid archive. |
| `not_available` | The archive version does not define this guarantee. |
| `not_applicable` | The archive version has no such structure or validation stage. |
| `not_completed` | An earlier failure prevented execution of this check. |

V1 reports `chunk_table` as `not_applicable` and always reports
`per_chunk_sha256`, `global_sha256`, and `trailing_data` as `not_available`.
The frozen v1 container has no authenticated payload-end field independent of
its payload. V2 metadata is carried by its fixed header and table rather than
a bincode object; successful validation reports its `metadata` status as
`passed`. Neither `not_applicable` nor `not_available` is equivalent to
`passed`: `not_applicable` means that the stage is not part of that format,
while `not_available` records a guarantee that the format cannot supply.

## `against`

| Field | JSON type | Meaning |
|---|---|---|
| `status` | string | `not_requested`, `matched`, `mismatched`, or `not_completed`. |
| `source_size_bytes` | integer or null | Size of the readable source when comparison ran; otherwise null. |

The comparison covers SHA-256 and length for the complete reconstructed and
source byte sequences. It is never sampled. `mismatched` makes the root
`valid` false even when all format-native checks passed. `not_completed` means
archive validation failed before a requested comparison could finish.

## `diagnostics`

Each diagnostic has exactly these fields:

| Field | JSON type | Meaning |
|---|---|---|
| `code` | string | Stable machine-readable failure category. |
| `severity` | string | `"error"` in V1. |
| `message` | string | Human-readable context; not a stable branching key. |

V1 diagnostic codes and triggers are:

| Code | Trigger |
|---|---|
| `ARCHIVE_HEADER_INVALID` | Magic or the minimum fixed header cannot be identified safely. |
| `UNSUPPORTED_ARCHIVE_VERSION` | A readable `.dpack` version is neither frozen version 1 nor version 2. |
| `V1_HEADER_OR_METADATA_INVALID` | A recognized v1 archive has invalid file-type, metadata-length, metadata-decoding, metadata-consistency, or extension fields. |
| `V1_PAYLOAD_INVALID` | A v1 zstd or DCSV01 payload cannot be decoded under the format bounds. |
| `V2_HEADER_OR_CHUNK_TABLE_INVALID` | A recognized v2 archive has an invalid mode, header, table, range, coverage, gap, overlap, or trailing-data condition. |
| `DECLARED_OUTPUT_LIMIT_REACHED` | The declared restored size exceeds `--max-output-mb`. |
| `CHUNK_COUNT_LIMIT_REACHED` | A v2 archive declares more chunks than `--max-chunks` permits. |
| `VALIDATION_MEMORY_LIMIT_REACHED` | The applicable v1 columnar or v2 validation estimate exceeds `--max-memory-mb`, or a bounded read cannot stay within it. |
| `CHUNK_DECOMPRESSION_FAILED` | A v2 compressed chunk cannot be read, bounded, or decompressed as declared. |
| `CHUNK_HASH_MISMATCH` | A reconstructed v2 chunk does not match its stored SHA-256. |
| `RESTORED_LENGTH_MISMATCH` | A v1 reconstruction, a v2 chunk, or the complete v2 byte count differs from the declared size. |
| `GLOBAL_HASH_MISMATCH` | The complete reconstructed v2 byte sequence does not match its stored global SHA-256. |
| `AGAINST_MISMATCH` | Complete reconstructed and source size/SHA-256 identities differ. |

Codes identify the stable stage of failure. The diagnostic `message` may add
bounded numeric details but must not disclose either input path or user data.

## Complete v2 example

```json
{
  "schema_version": 1,
  "report_type": "validation",
  "valid": true,
  "archive": {
    "version": 2,
    "format": "dpack_v2",
    "archive_size_bytes": 1542,
    "original_size_bytes": 588,
    "payload_mode": "chunked_raw_zstd",
    "chunk_count": 10
  },
  "checks": {
    "header": "passed",
    "metadata": "passed",
    "payload_structure": "passed",
    "decompression": "passed",
    "restored_length": "passed",
    "chunk_table": "passed",
    "per_chunk_sha256": "passed",
    "global_sha256": "passed",
    "trailing_data": "passed"
  },
  "against": {
    "status": "not_requested",
    "source_size_bytes": null
  },
  "diagnostics": []
}
```

## Complete v1 example

```json
{
  "schema_version": 1,
  "report_type": "validation",
  "valid": true,
  "archive": {
    "version": 1,
    "format": "dpack_v1",
    "archive_size_bytes": 331,
    "original_size_bytes": 810,
    "payload_mode": "raw_zstd",
    "chunk_count": null
  },
  "checks": {
    "header": "passed",
    "metadata": "passed",
    "payload_structure": "passed",
    "decompression": "passed",
    "restored_length": "passed",
    "chunk_table": "not_applicable",
    "per_chunk_sha256": "not_available",
    "global_sha256": "not_available",
    "trailing_data": "not_available"
  },
  "against": {
    "status": "not_requested",
    "source_size_bytes": null
  },
  "diagnostics": []
}
```

## Compatibility and evolution

Consumers must use `schema_version` and `report_type` before reading the rest
of the object. They should treat diagnostic messages as presentation and use
diagnostic codes and status strings for automation.

Adding a new optional diagnostic code does not change the meaning of existing
fields. Removing a field, changing a field's type, changing a status meaning,
or reusing an existing diagnostic code for a different condition requires a
new schema version.

Validation JSON versioning is independent of `.dpack` wire-format versioning.
This report does not add metadata to an archive or change v1 or v2 bytes.
