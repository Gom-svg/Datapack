# Matriz de caracterización de los analizadores

Estado: baseline de RFC-001B0. Este documento registra comportamiento real; no
redefine el planner, el parser ni los formatos `.dpack`.

## Autoridad actual por ruta

| Ruta | Autoridad actual | No es autoridad para |
|---|---|---|
| `planning::analyze_path` / `SampleAnalyzer` | Muestra, `ColumnProfile` y `CompressionPlan` usados por `analyze`, `compress` v1 no chunked y `benchmark` | Aceptabilidad y reconstrucción byte-exacta del payload DCSV01 |
| `analysis::analyze_bytes` | API pública legacy full-buffer que devuelve `DpackMetadata` y `CsvAnalysis` | Planner o selección de compresión del CLI |
| `formats::csv::columnar::{CsvSafetyScanner, encode, decode}` | Semántica simple/RFC 4180, aceptabilidad y reconstrucción byte-exacta de DCSV01 | Recomendación del planner |
| `storage::encode_columnar_dictionary_archive_detailed` | Conversión de un fallo del candidato columnar en `None` más diagnóstico | Selección RawZstd final por sí sola |
| `storage::encode_raw_zstd_archive` | Candidato/fallback v1 lossless | Clasificación estructural del dataset |

Las pruebas no fuerzan igualdad entre rutas con contratos distintos. Afirman
hechos y divergencias para que cualquier cambio durante RFC-001B sea explícito.

## Matriz del corpus

| Fixture / caso | `SampleAnalyzer` | `analysis::analyze_bytes` | Parser/códec columnar | Diferencia observada | Impacto | Esperado durante RFC-001B |
|---|---|---|---|---|---|---|
| `Repetitive`, 10.000 filas, seed 42 | Muestra completa de 544.597 B; 10 columnas `Dictionary`; recomienda `CsvColumnarDictionary`, 81,22726% y 0,009765625 MiB | No se ejecuta en esta regresión de política | Candidato válido y byte-exacto | RawZstd=76.493 B; columnar=28.925 B; planner elige columnar | Caso positivo estable | Conservar muestra, perfiles, estimaciones, motivos y elección |
| `Realistic`, 10.000 filas, seed 2026 | Muestra completa de 1.547.543 B; 10 columnas `Dictionary`, 2 `Raw`; recomienda columnar, 46,09023% y 0,408203125 MiB | No se ejecuta en esta regresión de política | Candidato válido y byte-exacto | RawZstd=298.855 B; columnar=206.276 B; planner elige columnar | Caso realista positivo | Conservar cardinalidades, censura, estrategias, motivos y elección |
| `HighCardinality`, 10.000 filas, seed 7 | Muestra completa de 2.996.518 B; 10 columnas censuradas, repetición 0, estrategia por columna `Raw`; recomienda `RawZstd` | No se ejecuta en esta regresión de política | Candidato válido y byte-exacto | RawZstd=1.507.722 B; columnar=1.269.920 B, pero el planner elige RawZstd | La política no equivale a comparar tamaños reales | Preservar la decisión legacy; no “optimizar” la heurística en B1 |
| `Random`, 10.000 filas, seed 99 | Muestra completa de 1.369.846 B; 8 columnas censuradas, repetición 0; recomienda `RawZstd` | No se ejecuta en esta regresión de política | Candidato válido y byte-exacto | RawZstd=1.040.834 B; columnar=1.002.499 B, pero el planner elige RawZstd | Mismo desacople entre estimación y candidato real | Preservar la decisión legacy; una mejora requiere RFC posterior |
| RFC 4180 con coma citada en una línea física | 78 B/3 filas; `DeltaCandidate`, `Dictionary`, `Dictionary`; recomienda RawZstd por coste estimado | Su split simplificado no es autoridad para quotes | Candidato válido; round-trip exacto | RawZstd=117 B y columnar=148 B; las quotes forman parte de longitudes/hashes del planner | Fija el subconjunto RFC que el planner sí tolera | Conservar elección, estimaciones y parse line-local |
| CSV válido no favorable (`id,value`) | 45 B/4 filas; `DeltaCandidate` y `Plain`; recomienda RawZstd | Analizable, pero no gobierna el plan | `encode` produce un payload válido y byte-exacto | RawZstd=98 B y columnar=131 B; “válido” no significa “recomendado” | Evita confundir aceptabilidad con política | Mantener separadas validez del códec y recomendación |
| Sin header (`1,Ada,active...`) | Consume la primera fila como nombres `1`, `Ada`, `active`; observa 1 fila | `has_headers=false`, 2 filas y nombres `column_1..3` | `Simple`; round-trip exacto | Dos políticas incompatibles de header sobre los mismos bytes | Cambiarla alteraría CLI/planner o API legacy | Mantener ambas hasta una decisión explícita de header |
| Record citado multilínea de 64 KiB | Falla en la primera línea física con `unterminated quoted field` | Cuenta 3 líneas físicas de datos bajo el header | `RequiresRfc4180`; `encode/decode` conserva el record lógico | Planner line-oriented frente a códec RFC-aware | `analyze`/`compress` v1 pueden rechazar algo codificable | No sustituir parser ni cambiar fallback/error en B1 |
| Ancho inconsistente | Falla al observar una fila con menos columnas | Analiza 2 filas y rellena el valor ausente al construir estadísticas | Scanner `Unsupported`; `encode` falla; storage devuelve `None + reason`; RawZstd restaura | La API pública es permisiva donde planner y códec rechazan | Una unificación ingenua cambiaría errores y decisiones | Congelar las tres respuestas y el fallback |
| Header de 32 KiB con budget artificial de 64 B | Lee 32.774 B, produce 2 perfiles y 0 filas; excede el budget | No se ejecuta | No se ejecuta | `max_bytes` no limita el primer `read_line` | Riesgo de memoria por header gigante | Documentar; no introducir nueva política en B0 |
| Record final de 32 KiB sin terminador, budget 64 B | Lee y cuenta el record completo; `sampled_bytes` equivale al archivo y excede el budget | No se ejecuta | `Simple`; conserva la ausencia de newline final | La primera fila de datos también puede sobrepasar el budget | Riesgo de memoria por record gigante | Documentar; diseñar límite explícito en RFC posterior |
| 1.024 columnas y una fila | Mantiene 1.024 perfiles por índice, sample completo y plan RawZstd | No se ejecuta | No se ejecuta | Coste de estado crece con anchura | Archivo extremo puede consumir memoria por columna | Mantener identidad por índice; no prometer anchura ilimitada |
| Delimitador ambiguo coma/pipe | Usa coma fija: headers `a`, `b|c` | El detector elige pipe: headers `a,b`, `c` | Con pipe resulta `Simple` y round-trip exacto | Dialecto distinto sobre los mismos bytes | `analyze` y el códec pueden describir columnas diferentes | Conservar coma fija del planner en B1; resolver dialecto después |

## Contratos de las pruebas

- Tamaños, filas, columnas, nombres, cardinalidades, estrategias, razones,
  candidatos y bytes restaurados se comparan exactamente.
- `estimated_savings_percent`, `estimated_memory_mb` y tasas `f32` usan una
  tolerancia absoluta de `0,01`.
- `planning_time_ms` no se congela.
- Un `unique_count` igual a filas después de `exceeded_cardinality` es el
  sentinel conservador actual; no se documenta como cardinalidad exacta.
- Que el candidato columnar sea más pequeño en `Random` y `HighCardinality` no
  autoriza cambiar `PlannerPolicyV1` dentro de RFC-001B.
- Las pruebas de límites usan 32 o 64 KiB y 1.024 columnas para recorrer las
  mismas ramas peligrosas sin poner en riesgo CI.

## Criterio de autoridad durante RFC-001B

Una extracción es compatible cuando el núcleo extraído produce para el adapter
`PlannerPolicyV1` los mismos hechos consumidos hoy por `build_plan`, las mismas
estrategias, razones, estimaciones, errores y cobertura de muestra. Por separado,
el códec debe conservar `Ok(Some)`, `Ok(None)` o `Err` y su reconstrucción
byte-exacta. Una divergencia solo puede eliminarse mediante una decisión
explícita posterior, no ajustando tests hasta forzar igualdad.
