# RFC-001B0 — Reporte de baseline de compatibilidad y caracterización

- Estado: completado, pendiente de revisión humana
- Fecha: 2026-07-29
- Alcance: formato, fixtures, pruebas, infraestructura de pruebas y documentación
- Recomendación: **Go condicionado** para RFC-001B

## 1. Resumen ejecutivo

RFC-001B0 deja una referencia ejecutable antes de extraer el Intelligence
Engine. El baseline anterior de 173 pruebas y todos sus quality gates estaba
verde. El baseline final contiene 196 pruebas: añade archivos `.dpack`
congelados v1/v2, restauración y SHA-256 byte-exactos, goldens del CLI,
decisiones detalladas de `PlannerPolicyV1`, divergencias entre los tres caminos
de CSV y límites de sampling.

No se modificaron encoders, decoders, metadata, wire formats, heurísticas,
dependencias, API pública ni rutas productivas del CLI. El único archivo bajo
`src/` modificado es `src/planning/tests.rs`, que es el módulo `#[cfg(test)]` ya
existente. La corrección inicial en `tests/security_hardening.rs` fue únicamente
rustfmt y quedó aislada.

Los `.dpack` creados son un baseline pre-refactor generado una sola vez con la
implementación inicial. No se presentan como artefactos de una release pública
anterior que no existe en el historial disponible. Desde este commit quedan
congelados y sus pruebas nunca los regeneran.

## 2. Estado base y procedencia

| Elemento | Revisión / valor |
|---|---|
| Código productivo auditado | `7a51af4` (`Initial Datapack project`) |
| Baseline rustfmt limpio | `fbcc6a9` (`style: restore clean rustfmt baseline`) |
| RFC-001A versionado | `ebcc2a6` (`docs: add intelligence engine architecture audit`) |
| Toolchain ejecutado | `rustc 1.97.0 (2d8144b78 2026-07-07)`; `cargo 1.97.0 (c980f4866 2026-06-30)` |
| Dependencias | `Cargo.toml` y `Cargo.lock` sin cambios |
| Tests antes de B0 | 173: 73 unitarios, 31 `round_trip`, 69 `security_hardening` |
| Tests después de B0 | 196: 83 unitarios, 9 `analyze_cli`, 4 `compatibility_fixtures`, 31 `round_trip`, 69 `security_hardening` |

Antes de añadir las nuevas pruebas se ejecutaron y pasaron `cargo fmt`,
`cargo fmt --check`, `cargo check`, `cargo test` con 173 pruebas y
`cargo clippy --all-targets --all-features -- -D warnings`.

## 3. Fixtures de compatibilidad congelados

El manifiesto de procedencia y opciones vive en
`tests/fixtures/compatibility/MANIFEST.md`. `.gitattributes` fuerza LF para las
fuentes byte-exactas y desactiva normalización textual para `.dpack`, evitando
que `core.autocrlf` altere hashes en otro checkout.

| Archive | Formato / estrategia | Fuente (bytes; SHA-256) | Archive (bytes; SHA-256) | Cobertura |
|---|---|---|---|---|
| `v1_raw_zstd.dpack` | v1 / `RawZstd` | 810; `81f5cc20f74dd9bea672f7bdbb5ecc1bf5fdb5379223c4af8c2cb650f54fb994` | 331; `7d467f55a0927c0fafc0a75c1fdf7be5f7ad2db430a8bee65dc9312c71ddadab` | Header y metadata bincode v1, zstd y restore exacto |
| `v1_csv_columnar.dpack` | v1 / `CsvColumnarDictionary` | 1269; `320c19793eea868aa109cb798887f92394de114879a3d5df3ed55fb1f49d16bf` | 265; `0210e1b969e124731cbe374fb030fc75e391949e588195b676b2a7a2b36070a5` | DCSV01 con LF final, ceros iniciales, vacíos, comas citadas y comillas escapadas |
| `v2_chunked_multichunk.dpack` | v2 / chunked `RawZstd` | 588; `f41051e6c9fcbf543208b67f26116e0f11188d997d52815f7ee24444f643f8f3` | 1542; `a0644b941c86dac0c0d0ccd37e8cd6f07fdd25d45f38672a0d0c626faa5f8cbd` | 10 chunks de 64 B, tabla, offsets, SHA por chunk, SHA global y restore actual |

`tests/compatibility_fixtures.rs`:

- lee sources y archives directamente desde el repositorio;
- comprueba tamaño y SHA antes de decodificar;
- confirma versión y `PayloadKind`;
- restaura y compara todos los bytes con la fuente;
- vuelve a hashear el archive tras la lectura para detectar mutación;
- prueba copias corruptas de v1 RawZstd, v1 columnar y v2, y confirma que no se
  instala output final ni queda `.partial`;
- no llama a ningún encoder para regenerar fixtures.

## 4. Goldens de `datapack analyze`

`tests/analyze_cli.rs` ejecuta el binario real mediante
`CARGO_BIN_EXE_datapack`. Hay nueve casos y diez archivos golden (un golden vacío
compartido):

| Caso | Comportamiento congelado |
|---|---|
| CSV simple con header y LF | stdout completo, columnas, estrategias y métricas |
| Repetitivo con CRLF y `--plan` | modo, ahorro, memoria, motivo y tabla completa |
| Sin header | la primera fila continúa tratándose como header legacy |
| Coma citada RFC 4180 en una línea | conteos, longitudes y estrategias actuales |
| Vacíos y UTF-8 | bytes, cardinalidad, repetición y longitudes actuales |
| Alta cardinalidad pequeña | columnas única/repetitiva y estrategias actuales |
| Una columna | exit 2 y stderr exacto |
| Archivo vacío | exit 2 y stderr exacto |
| Ancho inconsistente | exit 2 y stderr exacto |

Solo se normaliza el entero de una línea que coincide exactamente con
`Planning time:       <u64> ms`. El test exige una ocurrencia con `--plan` y cero
sin él. No se normalizan tamaños, filas, cardinalidad, estimaciones, razones,
exit codes ni ningún otro resultado funcional.

## 5. `PlannerPolicyV1` congelado

Las pruebas generan el corpus con seeds deterministas, congelan sample,
perfiles, razones y estimaciones, construyen ambos candidatos v1, validan sus
`PayloadKind`, restauran ambos byte-exactamente y registran sus tamaños.

| Dataset | Sample / clasificación | RawZstd | Columnar | Elección y dato relevante |
|---|---|---:|---:|---|
| Repetitive 10.000, seed 42 | 544.597 B, 10.000 filas, CSV válido, 10 `Dictionary` | 76.493 B | 28.925 B | `CsvColumnarDictionary`; ahorro estimado 81,22726% |
| Realistic 10.000, seed 2026 | 1.547.543 B, 10.000 filas, 10 `Dictionary` + 2 `Raw` | 298.855 B | 206.276 B | `CsvColumnarDictionary`; ahorro 46,09023% |
| HighCardinality 10.000, seed 7 | 2.996.518 B, 10 columnas censuradas/`Raw` | 1.507.722 B | 1.269.920 B | `RawZstd`; insuficiente repetición según política v1 |
| Random 10.000, seed 99 | 1.369.846 B, 8 columnas censuradas/`Raw` | 1.040.834 B | 1.002.499 B | `RawZstd`; insuficiente repetición según política v1 |
| RFC 4180 line-local | 78 B, 3 filas; Delta + 2 Dictionary | 117 B | 148 B | `RawZstd`; ahorro proyectado menor a 5% |
| CSV no favorable | 45 B, 4 filas; Delta + Plain | 98 B | 131 B | `RawZstd`; mayoría sin repetición suficiente |

La elección RawZstd para `HighCardinality` y `Random` se conserva aunque el
candidato columnar medido sea menor. La prueba protege la política actual; no
afirma que sea ideal y esta RFC no cambia la heurística.

## 6. Divergencias confirmadas

La matriz completa está en
`docs/testing/analysis-characterization-matrix.md`. Los resultados principales
son:

- `SampleAnalyzer` gobierna `analyze`, `compress` v1 no chunked y `benchmark`;
  `analysis::analyze_bytes` no gobierna esos comandos.
- Sin header, el planner consume la primera fila como nombres; la API pública
  conserva ambas filas y crea nombres ordinales; el códec reconstruye los bytes.
- Un record citado multilínea de 64 KiB es rechazado por el planner line-oriented,
  contado como líneas físicas por `analyze_bytes` y aceptado como RFC 4180 por el
  códec columnar.
- Un ancho inconsistente falla en planner y códec, pero la API pública llena
  ausencias durante su análisis; storage convierte el error columnar en fallback
  diagnosticado y RawZstd sigue siendo exacto.
- En un empate coma/pipe, el planner usa coma fija mientras detector, API pública
  y códec eligen pipe.
- La validez de un payload columnar no equivale a una recomendación columnar.

Estas diferencias son expectativas de caracterización durante RFC-001B, no una
propuesta de semántica final.

## 7. Sampling y memoria

Se añadieron regresiones seguras para CI que recorren las ramas de riesgo sin
crear inputs gigantes:

- header de 32 KiB con `max_bytes=64`: se leen 32.774 bytes y cero filas;
- primer record de 32 KiB sin terminador con `max_bytes=64`: se lee y cuenta
  completo, y `sampled_bytes` supera el budget;
- record citado multilínea de 64 KiB: el planner falla en la primera línea física
  y el códec completa round-trip;
- 1.024 columnas: se conservan perfiles separados por índice, sin panic;
- delimitador ambiguo: se congelan las decisiones distintas de dialecto.

No se midió RSS porque no sería determinista en CI. Los bytes inspeccionados se
afirman cuando `SampleAnalysis` los expone. No se agregó límite ni se cambió la
política existente.

## 8. Riesgos y bugs preexistentes no corregidos

### Bloqueantes para afirmaciones más amplias, no para la extracción compatible

1. `max_bytes` no es un límite duro: `read_line` materializa header o línea física
   completa antes de revisar el budget.
2. El coste de cardinalidad puede crecer como columnas por 8.192 hashes; 1.024
   columnas ya demuestra que la anchura multiplica estado.
3. Planner y códec no comparten parser; records multilínea cambian aceptabilidad.
4. La política de header y la detección de delimitador difieren entre rutas.
5. `ColumnPlan` y los límites de diccionario siguen sin gobernar completamente
   el modo elegido internamente por el encoder.

### Bugs/comportamientos concretos congelados

- El archivo vacío estable falla en `validate_csv_prefix` antes de alcanzar el
  branch `InvalidCsv("empty file")`; el CLI muestra el mensaje redundante
  `file is not valid CSV: file is not valid CSV within first 4 KB`.
- El renderer imprime `>65535` al censurar después de 8.192 hashes, aunque ese
  umbral no fue observado como cardinalidad.
- Sin header, `SampleAnalyzer` pierde la primera fila como datos.
- Para `Random` y `HighCardinality` de este corpus, el plan RawZstd no coincide
  con el candidato medido más pequeño.

No se corrigió ninguno porque hacerlo cambiaría comportamiento fuera del alcance
de RFC-001B0.

### Cobertura histórica todavía ausente

El historial disponible solo tiene el commit inicial, por lo que no fue posible
obtener `.dpack` de una release anterior real. También faltan fixtures v1 de
metadata rica y de variantes legacy `PayloadKind::Plain`/`Dictionary`. Esta
limitación no bloquea una extracción interna compatible con el baseline actual,
pero sí bloquea afirmar cobertura completa de artefactos de releases anteriores.

## 9. Quality gates reales

### Baseline antes de nuevas pruebas

| Comando | Resultado |
|---|---|
| `cargo fmt` | ejecutado; exit 0 |
| `cargo fmt --check` | pass; exit 0 |
| `cargo check` | pass; exit 0 |
| `cargo test` | pass; 173 passed, 0 failed |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass; exit 0 |

### Baseline final

| Comando | Resultado |
|---|---|
| `cargo fmt --check` | pass; exit 0 |
| `cargo check` | pass; exit 0 |
| `cargo test` | pass; 196 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass; exit 0 |
| `cargo test --test compatibility_fixtures` | pass; 4 passed, 0 failed |
| `cargo test --test analyze_cli` | pass; 9 passed, 0 failed |
| `cargo test planning::tests` | pass; 22 passed, 0 failed |

## 10. Invariantes de alcance comprobadas

- `.dpack` v1 y v2 no cambiaron: no se tocó código de storage, metadata ni
  codec; los fixtures de ambas versiones se leen y restauran.
- `compress` y la selección del planner no cambiaron: solo se añadieron asserts
  sobre la política existente.
- La salida legacy de `analyze` no cambió: ahora queda comparada completa.
- No se añadieron flags, API pública, JSON, TSV/PSV ni inferencia de tipos.
- No se sustituyó parser ni se unificaron analizadores.
- No cambiaron dependencias ni scripts.
- No aumentó memoria de producción: no cambió código productivo. Los casos de
  prueba usan tamaños deliberadamente pequeños.

## 11. Commits de RFC-001B0

| Commit | Propósito |
|---|---|
| `fbcc6a9` | `style: restore clean rustfmt baseline` |
| `f521ad6` | `test: add frozen v1 and v2 compatibility fixtures` |
| `f14f51e` | `test: characterize legacy analyze output` |
| `32c09ac` | `test: freeze PlannerPolicyV1 behavior` |
| `ac368d9` | `test: characterize analyzer divergence and sampling limits` |

`ebcc2a6` versiona RFC-001A, el documento de entrada de esta tarea; no contiene
un cambio funcional de B0.

## 12. Recomendación Go / No-Go

**Go condicionado** para RFC-001B, después de revisión humana de estos goldens,
hashes y decisiones congeladas.

El primer cambio de RFC-001B debería extraer un modelo factual interno y un
adapter explícito `PlannerPolicyV1`, demostrando igualdad contra este corpus.
Debe conservar `analysis::analyze_bytes` como API legacy y mantener separado el
códec RFC-aware. No debe empezar por sustituir parsers, corregir heurísticas o
publicar una API estable.

**No-Go** en RFC-001B para afirmar memoria estrictamente acotada, unificar
semántica de header/delimitador/multiline, cambiar la política o modificar wire
formats. Esos pasos requieren decisiones y tests adicionales posteriores.
