# RFC-001A — Auditoría y diseño del DataPack Intelligence Engine

- Estado: propuesta auditada; lista para revisión
- Fecha de auditoría: 2026-07-29
- Revisión auditada: `7a51af488ce4fadf8accb793c8b9e1f7f64c830c`
- Alcance: análisis y diseño; sin cambios de comportamiento ni de formato
- Recomendación: **Go condicionado** para RFC-001B, limitado a caracterización, extracción compatible y métricas pasivas

## 1. Resumen ejecutivo

DataPack ya tiene un planner compartido por `analyze`, la compresión v1 no chunked y `benchmark`, pero ese planner no vive en el módulo público `analysis`. La fuente real es el módulo privado `planning`, concretamente `SampleAnalyzer`, `ColumnProfile`, `SampleAnalysis` y `build_plan` en `src/planning/mod.rs` y `src/planning/plan.rs`.

La auditoría encuentra **tres caminos de análisis/parsing con reglas distintas**:

1. `src/planning/mod.rs`: sampling acotado nominalmente, header obligatorio de facto, coma fija y records por línea física. Es la fuente real del CLI y del planner.
2. `src/analysis/mod.rs` junto con `src/formats/csv/mod.rs`: API pública full-buffer, clasificada por extensión, con `String::from_utf8_lossy`, `lines()` y `split(delimiter)`. No tiene consumidores internos.
3. `src/formats/csv/columnar.rs`: parser byte-oriented usado por el codec `CsvColumnarDictionary`, con camino simple y camino `parse_rfc4180_rows`, soporte de campos citados y records multilínea, reconstrucción byte-exacta y validación final contra los bytes originales.

La conclusión principal es que el Intelligence Engine **no debe construirse sobre `analysis::analyze_bytes` ni añadir un cuarto analizador**. Debe extraer y caracterizar primero la semántica que hoy gobierna el planner, separar hechos de política de selección y, después, mover el scanner delimitado byte-oriented a una infraestructura común consumida por análisis y codec.

También hay una divergencia importante entre plan y ejecución: `apply_dictionary_limits` modifica el plan por columna, pero `compress` solo pasa `plan.archive_mode` al encoder. El encoder vuelve a elegir `Plain` o `Dictionary` mediante `choose_column_mode`. Por tanto, los límites por columna y `ColumnPlan.strategy` son actualmente recomendaciones declarativas; no controlan la representación escrita.

La propuesta de este RFC:

- congela `.dpack` v1 y v2, incluido `DpackMetadata` serializado con bincode;
- conserva exactamente la salida textual y la selección del planner actual durante la extracción;
- hace que `analyze`, `compress` v1 y `benchmark` consuman un único resultado de análisis;
- mantiene `compress --chunked` como v2 RawZstd sin análisis innecesario;
- añade primero métricas pasivas de coste O(1) por valor, sin conectarlas a los umbrales del planner;
- introduce CSV/TSV/PSV/TXT delimitado primero como capacidad opt-in de análisis;
- pospone entropía, inferencia rica de fechas y cambios de compresión hasta que existan mediciones y barreras de regresión.

La recomendación es **No-Go** para una reescritura completa de `analyze`, para publicar ya una API semánticamente estable, para cambiar el parser que usa `compress` o para habilitar nuevos delimitadores en la compresión. Es **Go condicionado** para RFC-001B si este se limita a tests de caracterización, un modelo factual interno, un adaptador de política `PlannerPolicyV1` y métricas pasivas.

## 2. Alcance, método y no objetivos

Esta auditoría inspeccionó el código de entrada, biblioteca, CLI, análisis, planner, formatos CSV/TXT, codec columnar, metadata, storage v1/v2, errores, pruebas, fuzz targets, documentación y scripts de verificación. Los nombres y rutas que se describen como estado actual existen en la revisión indicada arriba. Las rutas marcadas como “propuestas” todavía no existen.

No se implementa en RFC-001A:

- una reconstrucción del comando `analyze`;
- un cambio de parser o de umbrales;
- JSON para `analyze`;
- soporte de compresión para TSV/PSV;
- JSONL, logs libres o SQL dumps;
- un cambio de metadata o wire format;
- una nueva estrategia de compresión;
- inferencia semántica avanzada o modelos ML.

Solo se añade este documento. La ejecución puntual de `datapack analyze samples/repeated_values.csv --plan` se usó como comprobación de la ruta auditada; no se modificaron fuentes Rust.

## 3. Estado actual real

### 3.1 Estructura de módulos

`src/lib.rs` exporta públicamente:

- `analysis`
- `cli`
- `compression`
- `encoding`
- `error`
- `formats`
- `generation`
- `metadata`
- `storage`
- `tuning`

El módulo `planning` se declara con `mod planning;`, por lo que es privado aunque varios de sus tipos internos estén escritos como `pub`. Esto explica por qué la API que realmente usa el CLI no está disponible a consumidores de la librería.

| Ruta real | Responsabilidad actual | Observación para Intelligence Engine |
|---|---|---|
| `src/main.rs` | Parsea `Cli`, llama `cli::run`, imprime errores y decide exit code | Debe seguir siendo un adaptador mínimo. |
| `src/lib.rs` | Define la superficie de módulos de la crate | `analysis` es público; `planning` es privado. |
| `src/cli/mod.rs` | Define comandos, orquesta I/O, benchmark y renderiza texto/JSON manualmente | Contiene `analyze_command` y `print_planning_analysis`; presentación y orquestación están acopladas en un archivo grande. |
| `src/analysis/mod.rs` | `analyze_bytes(path, bytes) -> DpackMetadata` | API pública paralela, full-buffer y no consumida por el CLI, planner ni storage actual. |
| `src/planning/mod.rs` | Sampling, parsing comma-only, acumulación de columnas, proyección y construcción del plan | Fuente efectiva de `analyze`, `compress` v1 y `benchmark`. |
| `src/planning/plan.rs` | `ArchiveMode`, `ColumnStrategy`, `ColumnPlan`, `CompressionPlan` | Modelo de recomendación actual; todavía privado hacia fuera de la crate. |
| `src/formats/csv/mod.rs` | `CsvAnalysis`, análisis completo simplificado, detección de delimiter/newline/header | Sus tipos forman parte de metadata v1; sus reglas no son las del planner. |
| `src/formats/csv/columnar.rs` | Codec DCSV01, scanner de seguridad, parse simple/RFC-aware, elección real de columnas, reconstrucción | Contiene la base técnica más cercana a un scanner común byte-oriented, pero está acoplada al codec y trabaja full-buffer. |
| `src/formats/txt/mod.rs` | Análisis full-buffer de líneas y trigramas repetidos | Solo lo usa `analysis::analyze_bytes`; no debe mezclarse con el primer núcleo delimitado. |
| `src/metadata/mod.rs` | `FileType`, `DpackMetadata`, `PayloadKind` y metadata serializable | Es parte del wire format v1 y debe congelarse. No es el modelo nuevo de reportes. |
| `src/storage/mod.rs` | Lectura/escritura v1, RawZstd y DCSV01, límites de restauración | Los encoders usados por el CLI crean metadata mínima; `encode_archive`/`write_archive` aceptan metadata rica del caller. |
| `src/storage/chunked.rs` | Contenedor y pipeline RawZstd v2 acotado | `compress --chunked` no usa planner y debe conservarlo así. |
| `src/error/mod.rs` | `DatapackError`, `Result` y exit codes | El futuro API necesita errores de análisis más precisos sin cambiar exit codes legacy por defecto. |
| `tests/round_trip.rs` | Integración de compresión, benchmark y v1/v2 | No invoca `datapack analyze`. |
| `tests/security_hardening.rs` | Corrupción, límites, atomicidad y errores de v1/v2 | Las pruebas llamadas “existing” generan archivos con el encoder actual; no son fixtures históricos. |
| `src/planning/tests.rs` | Casos del planner y selección | Es la cobertura principal a convertir en caracterización explícita. |
| `benches/size_placeholder.rs` | Placeholder de zstd | No mide planner, parser, DataPack ni memoria. |

La prohibición de `unsafe` está activa en `src/lib.rs` y `src/main.rs` mediante `#![forbid(unsafe_code)]`.

### 3.2 Dos modelos de análisis y tres parsers

El nombre `analysis` induce a error arquitectónico. El camino público actual es:

```text
analysis::analyze_bytes(path, bytes)
  -> metadata::FileType::from_path(path)
  -> formats::csv::analyze(bytes)     para extensión .csv
  -> formats::txt::analyze(bytes)     para .txt, .log o Unknown
  -> DpackMetadata
```

No se encontró ninguna llamada interna a `analysis::analyze_bytes`. Los encoders que usa el CLI (`encode_raw_zstd_archive`, el camino columnar y el writer RawZstd streaming) construyen `DpackMetadata::minimal`. En cambio, las APIs genéricas `encode_archive` y `write_archive` serializan el `DpackMetadata` suministrado por el caller y pueden producir metadata rica; esa superficie pública también debe preservarse.

El analizador público legacy materializa el archivo y las columnas, `dictionary_columns` queda siempre vacío y la variante `EncodingStrategy::Rle` no se selecciona. `formats::txt::analyze` también es full-buffer y conserva mapas de líneas/frases. Estas conductas necesitan tests de caracterización antes de tocar `src/analysis/mod.rs`, aunque no sean el núcleo recomendado.

El camino real del planner es:

```text
planning::analyze_path(path, sample_mb)
  -> SampleConfig::from_sample_mb
  -> SampleAnalyzer::analyze_path
  -> ColumnProfile[]
  -> build_plan
  -> SampleAnalysis {
       input_name, total_file_size, sampled_bytes,
       sampled_rows, columns, plan
     }
```

El codec usa otro parser:

```text
storage::encode_columnar_dictionary_archive_detailed
  -> formats::csv::columnar::encode
  -> detect_delimiter
  -> CsvSafetyScanner::scan
  -> CsvShape::parse
       -> parse_simple_rows o parse_rfc4180_rows
  -> choose_column_mode por columna
  -> decode_with_output_limit
  -> comparación restored == original
```

Esta separación produce diferencias observables:

| Tema | Planner real | `formats::csv::analyze` | Codec columnar |
|---|---|---|---|
| Entrada | `Path`, streaming por líneas | `&[u8]` completo | `&[u8]` completo |
| Delimitador | Coma fija | Detecta `,`, `;`, tab, `|` por conteo crudo | Usa el mismo detector simplificado |
| Header | Siempre primera línea | Heurística | No necesita distinguir header para reconstruir |
| Quotes | Alterna estado dentro de una línea | No los interpreta | Camino byte-oriented con quotes escapados |
| Newline citado | No soportado | No soportado | Soportado por el parse completo |
| UTF-8 | Exige prefijo UTF-8 y usa `String` | Conversión lossy | Opera sobre bytes |
| Cardinalidad | Hashes acotados a 8.192 por columna | `HashMap` exacto y no acotado | Diccionarios exactos full-buffer |
| Memoria | Nominalmente sampleada, con excepciones | No acotada al tamaño del archivo | Whole-file v1 |
| Resultado | `SampleAnalysis` + `CompressionPlan` | `DpackMetadata` con `CsvAnalysis` | Payload DCSV01 o fallback |

### 3.3 Metadata y formatos de archivo

`DpackMetadata` deriva `Serialize` y `Deserialize` y se serializa con bincode dentro del header v1 en `src/storage/mod.rs`. Contiene `Option<CsvAnalysis>` y `Option<TxtAnalysis>`. La API genérica puede producir metadata rica y, aunque no haya fixtures históricos en este repositorio que lo demuestren, se debe asumir que archivos v1 externos pueden contenerla.

Por tanto:

- no se deben añadir ni reordenar campos/variantes en el grafo serde/bincode transitivo de v1: `DpackMetadata`, `FileType`, `PayloadKind`, `ExtensionPoint`, `CsvAnalysis`, `ColumnAnalysis`, `EncodingStrategy`, `NewlineStyle`, `DictionaryColumnPreview`, `DictionaryEncoded`, `TxtAnalysis` y `PhraseCount`; también deben conservarse discriminantes y orden;
- no se debe reutilizar `FileType` para añadir `Tsv` o `Psv`;
- el nuevo modelo de análisis debe ser independiente del modelo de archivo;
- la salida JSON futura debe tener su propia versión de schema y no serializarse dentro de `.dpack`.

## 4. Mapa de dependencias y flujo real

### 4.1 Mapa general

```text
src/main.rs
  -> cli::run
      -> Analyze
          -> planning::analyze_path
          -> print_planning_analysis

      -> Compress v1 no chunked
          -> planning::analyze_path
          -> apply_dictionary_limits (solo copia del plan)
          -> encode_for_plan[_detailed]
              -> storage::encode_raw_zstd_archive
              o  storage::encode_columnar_dictionary_archive_detailed
                    -> formats::csv::columnar

      -> Compress v2 chunked
          -> storage::chunked
          -> no llama al planner

      -> Benchmark
          -> planning::analyze_path (64 MiB fijo)
          -> rama RawZstd streaming o rama columnar en memoria
          -> print_benchmark_table / print_benchmark_json

API pública paralela, sin consumidores internos:
analysis::analyze_bytes
  -> metadata::FileType::from_path
  -> formats::{csv, txt}::analyze
```

### 4.2 Flujo de `datapack analyze input`

1. `src/main.rs` obtiene `Cli` mediante Clap.
2. `cli::run` despacha `Command::Analyze { input, plan, sample_mb }` a `analyze_command` (`src/cli/mod.rs:493`).
3. `planning::analyze_path` construye `SampleConfig`. `sample_mb` debe estar entre 1 y 2048 MiB; el máximo de filas queda fijo en 10.000.
4. `validate_csv_prefix` abre el archivo y lee hasta 4 KiB en una lectura separada. Rechaza prefijo vacío, NUL, UTF-8 inválido o ausencia de una coma.
5. `SampleAnalyzer::analyze_path` consulta `std::fs::metadata` para obtener el tamaño total, abre de nuevo el archivo y crea un `BufReader` de 256 KiB.
6. Lee la primera línea física completa en un `String`, la trata siempre como header, la separa con `parse_csv_record_refs` usando coma y exige al menos dos columnas.
7. Crea un `ColumnState` por header.
8. Lee líneas físicas hasta llegar a 10.000 filas o al presupuesto de bytes. Las líneas vacías cuentan bytes, pero no filas. Cada fila no vacía debe tener el mismo ancho del header.
9. `ColumnState::observe` acumula longitud media incremental, bytes, éxitos de parsing numérico y hashes únicos hasta `UNIQUE_TRACKING_LIMIT = 8_192`.
10. `build_profiles` proyecta filas, cardinalidad y tamaños usando `total_file_size / sampled_bytes`; parte de esas proyecciones no se conserva en el resultado.
11. `recommend_strategy` recomienda `Dictionary`, `Plain`, `DeltaCandidate` o `Raw` por columna.
12. `build_plan` calcula ahorro y memoria estimados, y selecciona `ArchiveMode::CsvColumnarDictionary` o `ArchiveMode::RawZstd`.
13. `print_planning_analysis` renderiza directamente con `println!`.
14. `main` imprime cualquier error como `error: ...` en stderr y termina con `DatapackError::exit_code()`.

`--plan` **no activa el planner**: el plan siempre se calcula. La opción solo controla si se imprimen modo global, ahorro estimado, memoria, tiempo y razón.

Detalles semánticos que deben congelarse antes de extraer:

- la tabla se titula `Unique est`, pero imprime `ColumnProfile.unique_count` observado en la muestra, no `projected_unique`;
- cuando se intenta superar 8.192 hashes, el renderer imprime `>65535`, aunque ese umbral no fue observado;
- los campos citados se miden y hashean en su representación física, incluidas las comillas;
- el header elimina solo comillas exteriores, sin desescapar `""`;
- `looks_numeric_or_date` también usa substrings del nombre, incluida `id`;
- `planning_time_ms` interno empieza después de `validate_csv_prefix`;
- el ahorro negativo se clampa a 0%;
- un archivo de una sola columna es rechazado antes del análisis.

La ejecución observada sobre `samples/repeated_values.csv` seleccionó `CsvColumnarDictionary`, 20,8% de ahorro estimado y tres columnas de diccionario, coherente con este flujo.

### 4.3 Flujo compartido con `compress`

#### v1 no chunked

`compress_command_inner` ejecuta el mismo `planning::analyze_path`, clona el plan y aplica `apply_dictionary_limits`.

- Si el modo es `RawZstd` y no se requiere comparar candidatos, el archivo se comprime por streaming con buffers de 256 KiB y se conserva el wire format v1.
- Si el modo es columnar, `--verify-best` está activo o `--mode best` decide comparar porque `plan.estimated_savings_percent < 15.0`, el CLI carga el archivo completo con `read_all_buffered_progress`. `--verify-best` compara siempre los dos candidatos en la ruta v1 no chunked.
- `encode_for_plan_detailed` intenta el modo global. Si el codec columnar rechaza la entrada o falla la validación de reconstrucción, cae a RawZstd.
- La escritura usa `storage::output::TempOutput` y solo hace commit al final.

La ruta v1 columnar sigue siendo whole-file; la memoria acotada documentada para archivos grandes corresponde al camino RawZstd streaming y al pipeline v2. RFC-001A no cambia esa frontera.

#### v2 chunked

Si `CompressOptions::uses_chunked()` es verdadero, `compress_command_inner` retorna por la ruta `storage::chunked::encode_raw_zstd_chunked_file` antes de ejecutar el planner. Además de `--chunked`, activan esta rama `--chunk-size-mb`, `--threads`, `--max-in-flight-chunks`, `--backend`, `--adaptive-level` y, en `compress`, `--max-memory-mb`. El modo v2 es siempre RawZstd; `--mode`, sampling, límites de diccionario y `--verify-best` no gobiernan esa salida. Ejecutar Intelligence Engine aquí sin necesidad sería CPU e I/O extra y no debe convertirse en requisito.

#### Brecha plan/encoder

`apply_dictionary_limits` cambia `plan.columns`, pero `encode_for_plan` recibe solamente `ArchiveMode`. `formats::csv::columnar::choose_column_mode` vuelve a calcular el modo de cada columna por tamaño exacto con un mínimo de 5% de ahorro. También construye mapas completos para calcular y después escribir el diccionario.

Consecuencias actuales:

- el mensaje “switching to Plain” no garantiza que el encoder use `Plain`;
- `--max-dictionary-values` y `--max-dictionary-mb` no son límites efectivos de asignación del codec;
- `ColumnPlan.strategy` no es un plan ejecutable;
- planner y encoder usan dos modelos económicos diferentes.

La API pública `storage::encode_adaptive_archive` constituye otra ruta de selección: comprime RawZstd, intenta columnar solo para `FileType::Csv` y conserva el payload menor, sin consultar `planning::build_plan`. No es el camino planificado del CLI, pero debe caracterizarse y, a largo plazo, delegar en un contrato común o quedar documentada explícitamente como selección exhaustiva distinta de una recomendación sampleada.

RFC-001B debe caracterizar esta brecha, no corregirla silenciosamente. Hacer efectivos los límites puede cambiar bytes y ratio de archivos v1 nuevos, aunque no cambie la capacidad de leer v1; requiere una decisión posterior y pruebas de archivo seleccionado.

### 4.4 Flujo compartido con `benchmark`

`benchmark_command` siempre ejecuta `planning::analyze_path(&input, 64)` antes de aplicar `--max-input-mb`.

- `--estimate-only` renderiza `SampleAnalysis` como tabla o JSON manual, sin comprimir.
- Si el planner recomienda RawZstd, `benchmark_raw_zstd_streaming` comprime, restaura y hashea el prefijo por streaming.
- Si recomienda columnar, carga el prefijo medido completo en memoria, ejecuta el candidato, restaura y valida SHA-256.
- El reporte distingue `estimated_mode`, `selected_mode` y `plan_was_correct`.
- Tabla y JSON se generan manualmente desde `BenchmarkMetrics`; no existe un modelo serializable común con `analyze`.

Una peculiaridad relevante es que `--max-input-mb` limita la medición después de que el planner ya inspeccionó su propio prefijo de hasta 64 MiB. Cambiar ese orden puede cambiar el modo estimado y no pertenece a una extracción compatible.

### 4.5 Manejo de errores

El planner devuelve principalmente:

- `AnalyzeRead` cuando no puede obtener metadata o abrir el archivo;
- `InvalidCsv` para prefijo inválido, archivo vacío, ancho, quotes o mínimo de columnas;
- `Io` para errores de lectura, incluido UTF-8 inválido encontrado después del prefijo.

Los exit codes actuales son:

- `AnalyzeRead`: 1;
- `InvalidCsv`: 2;
- el resto: normalmente 1.

`compress` envuelve errores de operación en `OperationFailed` e informa si el output previo se preservó. `analyze` y `benchmark` no usan ese wrapper. La salida JSON futura debe mantener errores en stderr y no contaminar stdout.

## 5. Inventario de métricas

Las categorías de esta tabla describen su idoneidad para el nuevo núcleo, no solo si aparece un campo con nombre parecido.

| Métrica | Clasificación | Implementación real y precisión | Coste y decisión de producto |
|---|---|---|---|
| Tamaño del archivo | **ya existe y es reutilizable** | `SampleAnalysis.total_file_size`, obtenido con metadata; `DpackMetadata.original_size` también existe, pero pertenece al archivo. | O(1), exacto para la ruta. Debe vivir en `DatasetAnalysis`, sin reutilizar metadata wire. |
| Tipo o clasificación | **existe, pero está acoplada** | `FileType::from_path` clasifica solo por extensión y está serializado en v1. El planner solo valida “parece CSV con coma”. | Barato, pero insuficiente. Crear `DetectedFormat` separado y conservar `FileType` congelado. |
| Delimitador | **existe, pero está acoplada** | `formats::csv::detect_delimiter` cuenta `,`, `;`, tab y `|` en 20 líneas físicas, incluso dentro de quotes. El planner lo ignora. | Detección quote-aware puede hacerse en el mismo scan. Debe informar ambigüedad; no elegir con confianza ficticia. |
| Newline | **existe, pero está acoplada** | `detect_newline_style` devuelve CRLF si encuentra cualquier par; el parser columnar lleva estado más estricto. No forma parte de `SampleAnalysis`. | O(n) sobre bytes ya inspeccionados, sin asignación. Registrar LF, CRLF, mixed, bare CR y none en un modelo nuevo. |
| Número de columnas | **existe solo como dato interno** | Es `SampleAnalysis.columns.len()` o `CsvShape.column_count`; no hay un campo factual explícito. El planner deriva ancho del header supuesto. | O(1) después del primer record. Debe ser explícito y acompañado de estabilidad/ancho máximo observado. |
| Filas observadas | **ya existe y es reutilizable** | `sampled_rows`, excluye header supuesto y líneas vacías. | O(1), pero debe llamarse records observados y documentar exactamente exclusiones y scope. |
| Filas estimadas | **existe solo como dato interno** | `projected_rows` es un operando descartado del modelo de costes, no una estimación pública reutilizable. Supone distribución estacionaria y, con cero filas observadas, usa `row_scale = 1`. | Una estimación pública debe definirse de nuevo con método/cobertura explícitos; no se debe exponer directamente `projected_rows` ni presentarlo como conteo real. |
| Cardinalidad por columna | **existe, pero está acoplada** | Sigue hashes distintos hasta 8.192; después marca `exceeded_cardinality`, vacía el mapa y `unique_count()` devuelve `sampled_rows.max(8_193)`. Ese sentinel puede sobreestimar lo demostrado; el único lower bound factual es 8.193, sujeto además a colisiones. | El cap evita crecimiento por columna, no global. El modelo factual debe usar `AtLeast(8_193)`; `PlannerFeaturesV1` conserva aparte el sentinel actual para no cambiar decisiones. |
| Porcentaje de vacíos | **debe implementarse** | No hay contador. | Un `u64` por columna y comparación por valor; coste mínimo. Definir si `""` quoted cuenta como vacío lógico o bytes no vacíos antes de activar parser nuevo. |
| Longitud promedio | **ya existe y es reutilizable** | `ColumnState.mean_len` incremental y `total_value_bytes`; se expone como `avg_value_len_bytes`. Mide representación física actual. | Coste O(1) por valor. Mantener “bytes” en el nombre y congelar semántica legacy para el planner. |
| Longitud mínima y máxima | **debe implementarse** | No se calculan. | Dos acumuladores por columna; coste despreciable. Métrica pasiva adecuada para la primera fase. |
| Porcentaje de repetición | **existe, pero está acoplada** | `1 - unique_count / sampled_rows`; se fuerza a 0 al censurar cardinalidad. `CsvAnalysis` tiene otro `repeated_value_count` full-buffer. | Útil para el planner actual, pero necesita estado exacto/censurado y denominador explícito. |
| Tipo de dato inferido | **existe solo como dato interno** | `numeric_success` prueba `i64` y `f64`; el nombre de columna puede producir `DeltaCandidate`. No existe un tipo inferido expuesto. | Entero/float/bytes básicos pueden agregarse en una pasada. Fechas, timestamps, locale y decimal deben ser opt-in o pospuestos. |
| Entropía aproximada | **no es recomendable implementar todavía** | No existe. | Un histograma de 256 bins por columna cuesta ~2 KiB con `u64`, multiplicado por ancho, y escanea todos los bytes. No se ha demostrado que mejore el planner. Evaluar primero entropía byte-level de muestra en benchmark. |
| Compresibilidad estimada | **existe, pero está acoplada** | `estimated_encoded_size`, `estimated_raw_size` y `estimated_savings_percent` modelan diccionario, no zstd; usan extrapolación de prefijo. | Útil como heurística compatible, no como promesa. Nombrar el método y no mezclarlo con ratio medido. Un trial zstd añadiría CPU y retención de bytes. |
| Estrategia recomendada | **ya existe y es reutilizable** | `ArchiveMode`, `ColumnStrategy`, `CompressionPlan` y la función pura `build_plan`. | Es la semántica a congelar como `PlannerPolicyV1`; primero permanece `pub(crate)`. |
| Memoria estimada | **existe, pero está acoplada** | `estimated_memory_mb` suma tamaños proyectados de diccionarios recomendados. Benchmark la etiqueta como peak memory, pero omite mapas, ancho, línea, buffers, zstd y encoder. | No debe llamarse memoria pico sin calificación. Separar `estimated_dictionary_bytes` de un futuro `working_set_lower_bound`. |
| Nivel de confianza | **debe implementarse** | No existe. | Puede derivarse sin otra pasada de cobertura, censura, ambigüedad de dialecto, errores, truncamiento y representatividad. Debe exponer factores, no solo un porcentaje arbitrario. |

Métricas de bajo coste adicionales que ya existen o conviene conservar son `sampled_bytes`, `planning_time_ms`, estado de cardinalidad excedida, ancho estable y presencia de newline final. Ninguna métrica nueva debe entrar en `build_plan` durante RFC-001B.

### 5.1 Semántica recomendada para confianza

No se recomienda un número “87%” sin calibración. La primera versión debería usar una clasificación cualitativa y razones estructuradas:

- `High`: scan completo, dialecto no ambiguo, records válidos y contadores no censurados;
- `Medium`: sample suficiente y consistente, pero no completo;
- `Low`: prefijo pequeño, distribución posiblemente sesgada, cardinalidad censurada o detección ambigua;
- `Unknown`: no se pudo establecer el contrato del formato.

Los factores deberían incluir, como mínimo, fracción de bytes inspeccionada, records observados, límite que detuvo el scan, número de columnas, estabilidad, cardinalidad censurada, dialecto ambiguo y nivel de validación. Esto permite cambiar la fórmula en el futuro sin fingir precisión estadística.

## 6. Arquitectura incremental propuesta

### 6.1 Invariantes

1. Existe un solo núcleo que produce hechos de análisis para los consumidores planificados (`analyze`, `compress` v1 y `benchmark`). `analysis::analyze_bytes` permanece como API legacy separada hasta que pueda adaptarse o deprecarse sin romper callers.
2. El planner es una política pura y versionada que consume esos hechos; el CLI no recalcula métricas.
3. La extracción inicial reproduce bit a bit las decisiones actuales de `build_plan` y los errores/salidas caracterizados.
4. Métricas nuevas son pasivas hasta que otro RFC autorice cambios de selección.
5. El scanner no conserva el archivo completo ni diccionarios sin un presupuesto explícito.
6. La identidad de columna es el índice; el nombre es metadata potencialmente vacío, duplicado o inválido.
7. La metadata de análisis no se serializa dentro de v1 ni v2.
8. El parser del codec conserva validación byte-exacta y fallback; un reporte nunca sustituye una validación de reconstrucción.

### 6.2 Flujo objetivo

```text
Path / Read
  -> AnalysisEngine
       -> bounded input + dialect/header policy
       -> DelimitedRecordScanner
       -> AnalysisAccumulator (una pasada de estadísticas)
       -> DatasetFacts
  -> PlannerPolicyV1::plan(&DatasetFacts)
  -> DatasetAnalysis { facts, recommendation, coverage, diagnostics }
       -> CLI text renderer
       -> versioned JSON report
       -> Rust API
       -> futura capa Python
```

En compresión:

```text
DatasetAnalysis
  -> CompressionPlan
  -> encoder
       -> transformación/validación necesaria del payload
       -> no vuelve a calcular estadísticas del reporte
```

El scan de transformación del encoder puede seguir siendo una segunda pasada física: retener todas las celdas para evitarlo violaría la memoria acotada. Lo que se prohíbe es una segunda **fuente de reglas o estadísticas**, no una segunda lectura necesaria para producir el payload.

### 6.3 Migración sobre la estructura real

Se proponen estas rutas nuevas, todavía inexistentes:

| Ruta propuesta | Justificación desde el código actual |
|---|---|
| `src/analysis/model.rs` | `analysis` ya es público, pero su único resultado es `DpackMetadata`. Aquí debe vivir el modelo independiente del wire. |
| `src/analysis/engine.rs` | Extrae la orquestación hoy enterrada en `SampleAnalyzer::analyze_path`. |
| `src/analysis/accumulator.rs` | Centraliza contadores de columna y presupuestos; evita recalcular métricas en comandos. |
| `src/formats/delimited/mod.rs` | Extrae gradualmente el scanner byte-oriented desde `csv::columnar`. Para streaming debe emitir eventos/chunks (`field_start`, bytes, `field_end`, `record_end`); solo el adaptador full-slice del codec puede devolver rangos contiguos. |
| `src/cli/analysis_report.rs` | Separa render de `src/cli/mod.rs` sin mover lógica de decisión al CLI. |

No es necesario crear todas las rutas en un solo commit. El orden seguro es:

1. mover tipos y funciones sin cambiar código ejecutado;
2. introducir un facade compartido con el parser legacy intacto;
3. añadir métricas pasivas;
4. extraer el scanner byte-oriented con tests diferenciales;
5. habilitar formatos opt-in solo en `analyze`;
6. conectar serialización versionada.

`src/planning/mod.rs` debe conservar la política de selección. Se recomienda renombrar conceptualmente sus umbrales a `PlannerPolicyV1`, aunque el primer commit puede mantener nombres para minimizar diff. `src/planning/plan.rs` puede seguir privado durante la incubación.

### 6.4 Una sola fuente de verdad sin cambiar compresión

La transición necesita dos capas explícitas:

- **hechos**: bytes/records observados, dialecto, cobertura, contadores y estado de censura;
- **política**: fórmulas y umbrales exactos actuales que producen el `ColumnProfile` legacy del planner y `CompressionPlan`.

Para preservar resultados, `PlannerPolicyV1` debe usar inicialmente un adaptador `PlannerFeaturesV1` con los mismos tipos, redondeos, sentinel de cardinalidad y peculiaridades actuales. El nuevo scanner RFC-aware no sustituye al parser legacy en los consumidores de compresión hasta que un test diferencial y un RFC de comportamiento autoricen el cambio.

Durante desarrollo se permite ejecutar legacy y nuevo motor en paralelo **solo dentro de tests o herramientas de diagnóstico**. Producción debe tener un único resultado autoritativo para evitar doble I/O y divergencia.

La elección exacta de `Plain`/`Dictionary` del codec debe extraerse más adelante a un modelo de costes compartido. Hasta entonces, el reporte debe distinguir:

- recomendación estimada del planner;
- modo final seleccionado por el encoder;
- fallback por seguridad/reconstrucción.

No se debe afirmar que `ColumnPlan` es ejecutable mientras el encoder no lo consuma.

## 7. Modelo de datos propuesto

Los nombres se ajustan a convenciones existentes. En particular, no se propone otro `ColumnAnalysis`, porque ya existe `formats::csv::ColumnAnalysis`. El `ColumnProfile` actual se conserva como DTO/adaptador legacy de la política; los hechos nuevos usan el nombre distinto `ColumnStatistics` para no mezclar medición con recomendación y tamaños proyectados.

### 7.1 Superficie conceptual

```rust
pub struct AnalysisOptions {
    pub scope: AnalysisScope,
    pub format_hint: FormatHint,
    pub header_mode: HeaderMode,
    pub validation_level: ValidationLevel,
}

pub enum AnalysisScope {
    BoundedSample { max_bytes: u64, max_records: u64 },
    FullScan,
}

pub enum ValidationLevel {
    Prefix,
    Sample,
    Complete,
}

pub enum DetectedFormat {
    Delimited(DelimitedFormat),
    PlainText,
    Unknown,
}

pub struct DelimitedFormat {
    pub delimiter: u8,
    pub newline: DetectedNewline,
    pub header: HeaderDisposition,
    pub column_count: usize,
}

pub struct DatasetAnalysis {
    pub source_size_bytes: u64,
    pub format: DetectedFormat,
    pub coverage: AnalysisCoverage,
    pub columns: Vec<ColumnStatistics>,
    pub recommendation: Option<CompressionPlan>,
    pub confidence: AnalysisConfidence,
    pub diagnostics: Vec<AnalysisDiagnostic>,
}
```

Es un boceto, no una promesa de API. `FullScan` significa scan streaming completo; no autoriza cardinalidad exacta sin límites ni retener el archivo. La recomendación puede ser `None` para `PlainText`, `Unknown` o análisis insuficiente; una alternativa futura sería representar explícitamente el fallback RawZstd.

### 7.2 Visibilidad y estabilidad

| Tipo | Visibilidad inicial | Visibilidad objetivo | Serialización |
|---|---|---|---|
| `AnalysisOptions` | `pub(crate)` durante extracción | `pub` tras estabilizar defaults/errores | Serializable como parte de metadata de reporte, no de `.dpack`. |
| `DatasetAnalysis` | `pub(crate)` | struct `pub` y `#[non_exhaustive]`, o campos privados con accessors | DTO versionado posterior. |
| `ColumnProfile` legacy | Mantener efectivo `pub(crate)` dentro de `planning` | `pub(crate)` | No como modelo factual; solo adapter de `PlannerPolicyV1`. |
| `ColumnStatistics` | `pub(crate)` | `pub` tras estabilizar exactitud/unidades | Posterior; cardinalidad debe expresar exactitud. |
| `DetectedFormat` / `DelimitedFormat` | `pub(crate)` | `pub` | Posterior; delimiter JSON debe tener representación estable, no un carácter ambiguo. |
| `AnalysisScope` | `pub(crate)` | `pub` | Sí, una vez definidos bytes/records y límites. |
| `ValidationLevel` | `pub(crate)` | `pub` | Sí, separado de `AnalysisConfidence`. |
| `CompressionPlan`, `ArchiveMode`, `ColumnStrategy` | Seguir en módulo privado | Reexportar solo cuando se documenten como recomendación, no garantía de payload | Serializable mediante schema de reporte posterior. |
| `PlannerFeaturesV1`, `PlannerPolicyV1` | `pub(crate)` | `pub(crate)` | No. Son compatibilidad interna. |
| Estado de parser, slices, hashes y acumuladores | privado | privado | No. |
| `AnalysisReportV1` propuesto | no existe | público o módulo `report` | Sí; contiene `schema_version` y es independiente de bincode. |

No se propone un enum único `RecommendedStrategy`: DataPack ya distingue correctamente el nivel global (`ArchiveMode`) del nivel de columna (`ColumnStrategy`). Duplicarlos introduciría vocabulario redundante. `DatasetAnalysis.recommendation` puede contener opcionalmente el `CompressionPlan` existente cuando su semántica esté documentada.

### 7.3 Columnas, headers y cardinalidad

El modelo público debe identificar columnas por índice. Un posible perfil factual es:

```rust
pub struct ColumnStatistics {
    pub index: usize,
    pub name: Option<String>,
    pub name_status: ColumnNameStatus,
    pub observed_values: u64,
    pub empty_values: u64,
    pub cardinality: CardinalityEstimate,
    pub length_bytes: LengthStatistics,
    pub repetition: Option<RateEstimate>,
    pub inferred_type: Option<InferredDataType>,
}

pub enum CardinalityEstimate {
    Exact(u64),
    AtLeast(u64),
}
```

Al censurarse el tracker actual, el hecho publicable es `AtLeast(8_193)`, no `AtLeast(sampled_rows)`. El adapter `PlannerFeaturesV1` conserva por separado `sampled_rows.max(8_193)` porque cambiar ese sentinel sí alteraría estimaciones/planes legacy.

Los nombres duplicados no fusionan perfiles. Un nombre vacío o inválido genera un display name como `column_3`, pero preserva `index = 2`. La API debe definir si acepta UTF-8 inválido; el scanner interno opera sobre bytes y no debe hacer conversión lossy silenciosa.

### 7.4 Separación de modelo y JSON

No se recomienda añadir `Serialize` directamente a todo el modelo en el primer commit. La salida JSON debe usar un DTO `AnalysisReportV1` con:

- `schema_version`;
- unidades en nombres (`*_bytes`, `*_percent`);
- `null` para métricas no disponibles;
- enums como strings estables;
- cobertura y precisión junto a cada estimación relevante;
- diagnostics estructurados;
- ningún campo de timing usado como golden determinista.

La futura API Python debe mapear el mismo `DatasetAnalysis` o su DTO versionado; no debe volver a parsear el archivo en Python.

## 8. Formatos delimitados iniciales

### 8.1 Contrato común

CSV con coma, TSV, PSV, punto y coma y TXT delimitado son el mismo problema de scanner con un delimitador ASCII de un byte y reglas de quoting configuradas. El núcleo debe parametrizar el delimiter, no crear un parser por extensión.

El scanner común debe emitir eventos incrementales de records/campos sin perder la secuencia de:

- bytes originales;
- quotes dobles escapados;
- delimitadores dentro de quotes;
- LF o CRLF dentro de quotes;
- newline de record;
- presencia de newline final;
- índice físico/lógico para diagnostics.

Para la fase posterior que introduzca delimiter configurable, este debe limitarse inicialmente a un byte ASCII no igual a quote, CR o LF. Delimitadores multibyte añaden complejidad sin apoyo en DCSV01 ni evidencia de producto.

### 8.2 Estrategia de adopción

1. **Coma legacy:** conservar comportamiento por defecto para `analyze`, `compress` y `benchmark` mientras se extrae.
2. **Override explícito en análisis:** permitir en una fase posterior tab, pipe, punto y coma u otro byte, sin cambiar compresión.
3. **Autodetección en análisis:** puntuar candidatos con records lógicos, estabilidad de ancho, número de records válidos y evidencia fuera de quotes.
4. **Ambigüedad explícita:** devolver candidatos/diagnostic y confianza baja; el override resuelve el empate.
5. **Compresión:** mantener RawZstd para formatos nuevos hasta que otro RFC demuestre cómo mapearlos al codec sin cambiar v1/v2.

El decoder DCSV01 actual ya acepta `,`, `;`, tab y `|`, pero `storage::encode_adaptive_archive` solo intenta columnar para `FileType::Csv` y el planner exige coma. Esa capacidad parcial no autoriza habilitar TSV/PSV de forma incidental.

### 8.3 Header

Se necesita `HeaderMode::{Present, Absent, Auto}`. Para compatibilidad, los consumidores legacy siguen con `Present` durante la extracción. El modo `Auto` debe ser una capacidad nueva y su incertidumbre debe aparecer en confidence/diagnostics.

Un archivo sin header no puede perder la primera fila. Sus nombres públicos deben ser ordinales estables. Un header duplicado sigue siendo válido como datos; la identidad por índice evita ambigüedad.

### 8.4 Fuera de alcance

JSONL, logs libres y SQL dumps no se consideran formatos delimitados en RFC-001A. `DetectedFormat` puede ser extensible, pero no se crean parsers, métricas ni heurísticas para ellos. `formats::txt::analyze` permanece legacy y separado.

## 9. Compatibilidad e invariantes de entrega

La propuesta confirma explícitamente:

| Invariante | Cómo se preserva |
|---|---|
| No cambia `.dpack` v1 | Todo el grafo serde/bincode enumerado en §3.3, el payload DCSV01 y el serializer quedan congelados. El modelo nuevo no entra al archivo. |
| No cambia `.dpack` v2 | `storage::chunked`, header, tabla, mode 1, hashes y orden no se modifican. |
| No altera el resultado de `compress` | RFC-001B extrae la semántica actual y usa tests diferenciales; métricas nuevas no alimentan el planner. Cambios de parser/delimiter en compresión quedan fuera. |
| No modifica la selección actual del planner | Los umbrales, tipos numéricos, redondeos y orden de decisiones se encapsulan como `PlannerPolicyV1`. |
| No rompe scripts existentes | El renderer textual por defecto se congela con golden tests. JSON se añade solo por flag nuevo. Exit codes legacy se caracterizan. |
| No aumenta accidentalmente memoria | RFC-001B conserva el comportamiento nominal actual, mide el overhead fijo de métricas pasivas y añade regresiones de recursos. Los límites duros nuevos solo pueden ser opt-in mientras cambien aceptación/errores; activarlos por defecto requiere otro RFC. |
| No crea diferencias entre comandos | `analyze`, v1 `compress` y `benchmark` llaman al mismo engine y policy. `compress --chunked` conserva su estrategia fija y no recalcula un plan inútil. |

Estas son condiciones de aceptación de futuros commits, no una afirmación de que el código actual ya cumpla una única fuente de verdad. Hoy existen las divergencias descritas.

## 10. Riesgos y mitigaciones

### 10.1 Doble parsing

Hoy puede haber validación de prefijo, sampling del planner, safety scan del codec, parse completo y reintento RFC. Además, `dictionary_column_size` y `write_dictionary_column` vuelven a construir estructuras.

Mitigación:

- una sola pasada estadística por comando;
- dialecto y cobertura se pasan al planner;
- el encoder conserva una pasada de transformación, pero no recalcula métricas del reporte;
- extraer un modelo de costes compartido;
- no retener la muestra solo para evitar I/O si ello rompe el presupuesto de memoria.

### 10.2 Estadísticas inconsistentes

`analysis::analyze_bytes`, planner y codec difieren en header, quotes, delimitador y cardinalidad.

Mitigación: marcar las APIs legacy, crear un único `DatasetAnalysis`, añadir tests diferenciales y evitar que el CLI compute porcentajes o nombres por su cuenta.

### 10.3 Sampling engañoso

El sample siempre es un prefijo y `byte_scale` supone estacionariedad. Un archivo con primeras 10.000 filas repetitivas y resto único puede producir una recomendación optimista.

Mitigación: cobertura explícita, confidence con razones, `AtLeast(8_193)` para la censura actual y, solo tras medir I/O, considerar muestreo estratificado seekable. El planner v1 conserva prefix sampling por compatibilidad.

### 10.4 Límite de memoria no duro

`read_line` lee el header o una línea física completa antes de comprobar `max_bytes`. Un record enorme puede superar arbitrariamente `--sample-mb`. Una línea que cruza el límite se suma a `sampled_bytes` aunque pueda descartarse sin observar sus campos; si es la primera fila de datos, incluso puede procesarse por encima del límite. El coste de hashes es `columnas × 8.192`, sin presupuesto global, y `HashMap::clear` puede conservar capacidad.

Mitigación objetivo: scanner incremental por chunks, máximo de bytes de record/header, máximo de columnas, presupuesto global de cardinalidad y estado truncado/diagnostic en vez de asignación no acotada. RFC-001B solo debe instrumentar/caracterizar y evitar regresiones; activar esos límites por defecto cambiaría aceptación/errores legacy y requiere otro RFC. Pueden ofrecerse primero como opciones explícitas.

Este riesgo es bloqueante antes de declarar que el nuevo engine tiene memoria estrictamente acotada.

### 10.5 Inferencia de tipos costosa o ambigua

El planner actual intenta `i64` y `f64`; fechas se infieren por substring de nombre. Fechas reales requieren formatos, timezones y locale.

Mitigación: conservar el feature numérico legacy solo para `PlannerPolicyV1`; exponer inicialmente `Integer`, `Float`, `Utf8/Bytes`, `Mixed` y `Unknown` si se implementa. Posponer fechas/timestamps y permitir desactivar inferencia.

### 10.6 Headers inválidos o duplicados

El planner acepta vacíos y duplicados; los mensajes por nombre pueden ser ambiguos. La conversión futura a mapas JSON podría perder columnas.

Mitigación: índice estable, lista ordenada, `ColumnNameStatus`, display name separado y diagnostics. Nunca usar nombre como key único del reporte.

### 10.7 Records multilínea RFC-aware

El codec puede procesarlos en el parse completo, pero el planner falla en la primera línea física con quote abierto. `CsvSafetyScanner` puede además cortar su muestra de 1 MiB en un newline que esté dentro de quotes.

Mitigación: scanner incremental con estado de quote entre buffers y fronteras de record lógicas. Antes de adoptarlo en compresión, comparar planes, errores y payloads con corpus legacy.

### 10.8 Archivos sin header

Hoy la primera fila se pierde como dato y se convierte en nombre.

Mitigación: `HeaderMode`, default legacy `Present` para compatibilidad, y `Auto/Absent` opt-in hasta decidir un default futuro.

### 10.9 Delimitadores ambiguos

El detector actual cuenta caracteres crudos y no representa empates. Delimitadores dentro de quotes sesgan el score.

Mitigación: puntuar únicamente separadores fuera de quotes, estabilidad, records válidos y ancho. Un empate devuelve baja confianza/diagnostic; no se elige silenciosamente para compresión.

### 10.10 Alta cardinalidad

El cap actual protege cada columna parcialmente, pero la semántica de `unique_count` y `>65535` es confusa. Hashes pueden colisionar y el encoder full-buffer no está limitado por esos flags.

Mitigación: `CardinalityEstimate::AtLeast(8_193)`, sentinel legacy separado, presupuesto global opt-in, algoritmo aproximado solo si se mide y límites efectivos del encoder en un RFC posterior.

### 10.11 Archivos extremadamente anchos

Un `ColumnState` y hasta 8.192 hashes por columna pueden multiplicar memoria; el header y el vector de fields también crecen.

Mitigación: `max_columns`, budget global, reserva fallible, diagnostics y tests con 10.000+ columnas. Esos límites deben ser opt-in hasta autorizar el cambio de defaults; el criterio futuro debe ser error limpio o perfil parcial, nunca panic/OOM.

### 10.12 UTF-8 y corte del prefijo

`validate_csv_prefix` puede cortar un code point justo en 4 KiB y rechazar un archivo UTF-8 válido. El codec trabaja en bytes, pero el planner usa `String`.

Mitigación: validar UTF-8 incrementalmente o mantener bytes hasta la capa de nombres. No usar `from_utf8_lossy` en el núcleo sin un diagnostic explícito.

### 10.13 Compatibilidad no congelada con fixtures históricos

No hay archivos `.dpack` comprometidos. Tests con nombres `existing_v1_archive_passes`, `existing_v2_archive_passes` y `v1_archive_still_decompresses_after_v2_support` generan el archive con el encoder actual durante el test. Encoder y decoder podrían cambiar juntos y el test seguir pasando.

Mitigación: obtener de una release previa fixtures v1 RawZstd, v1 columnar, v1 con metadata rica que ejercite el grafo serde transitivo y `PayloadKind::Plain`/`Dictionary` legacy, y v2 multichunk, junto con originales y SHA-256 congelados.

Este riesgo bloquea cualquier modificación de tipos serializados, aunque RFC-001B no debería modificarlos.

### 10.14 Plan por columna no ejecutable

Los límites y estrategias del plan no controlan `choose_column_mode`.

Mitigación: primero test que documente la brecha; después decidir entre convertir `CompressionPlan` en plan ejecutable o renombrar las columnas como recomendaciones. Hacer efectivos los límites requiere aceptación explícita de posibles cambios en archives nuevos.

## 11. Plan incremental de implementación

Cada elemento es un commit independiente propuesto. Las rutas nuevas se indican explícitamente.

### Commit 1 — Congelar comportamiento y compatibilidad

- **Objetivo:** capturar salida, errores, planes y archives antes del refactor.
- **Archivos probables:** `src/planning/tests.rs`, `tests/round_trip.rs`, nuevo `tests/analyze_cli.rs`, nuevos goldens bajo `tests/golden/`, nuevo `tests/archive_compat.rs` y fixtures históricos bajo `tests/fixtures/archives/`.
- **Pruebas:** `analyze` con/sin `--plan`, modos repetitivo/random/alta cardinalidad, errores legacy, payload final, fixtures v1/v2 y SHA; caracterización directa de la API pública `analysis::analyze_bytes` para CSV/TXT/Unknown y del `DpackMetadata`/serde resultante; una regresión representativa de recursos para sample ancho/línea grande sin imponer aún límites nuevos.
- **Riesgo:** tiempos no deterministas y fixtures generados con la versión equivocada.
- **Aceptación:** goldens normalizan solo `planning_time_ms`; fixtures tienen procedencia/version y se leen sin regenerarse.

### Commit 2 — Extraer el modelo factual interno sin cambiar parser

- **Objetivo:** extraer los contadores de muestra y perfiles actuales a un modelo independiente de CLI y metadata wire. `AnalysisCoverage` todavía es una métrica nueva, no un campo que ya exista en `SampleAnalysis`.
- **Archivos probables:** `src/analysis/mod.rs`, nuevo `src/analysis/model.rs`, `src/planning/mod.rs`, `src/lib.rs`.
- **Pruebas:** igualdad estructural de `SampleAnalysis`/plan antes y después sobre el corpus; mismos errores y límites; `analysis::analyze_bytes` conserva firma y resultados legacy.
- **Riesgo:** cambios de `f32/f64`, orden, timing o ownership alteran resultados.
- **Aceptación:** todos los goldens y decisiones son idénticos; no cambia la API pública existente ni `DpackMetadata`.

### Commit 3 — Introducir `AnalysisEngine` y `PlannerPolicyV1`

- **Objetivo:** un único facade para `analyze`, `compress` v1 y `benchmark`, conservando las fórmulas actuales.
- **Archivos probables:** nuevo `src/analysis/engine.rs`, `src/analysis/mod.rs`, `src/planning/mod.rs`, `src/cli/mod.rs`.
- **Pruebas:** test de consistencia de los tres consumidores; `benchmark --estimate-only`; modo/payload de `compress`.
- **Riesgo:** benchmark usa 64 MiB fijo y aplica `max_input_mb` después; una “limpieza” cambiaría semántica.
- **Aceptación:** mismos outputs, exit codes, archive mode y bytes de archives deterministas en la misma build.

### Commit 4 — Separar el renderer legacy

- **Objetivo:** que el CLI solo presente `DatasetAnalysis` y congelar stdout.
- **Archivos probables:** `src/cli/mod.rs`, nuevo `src/cli/analysis_report.rs`.
- **Pruebas:** goldens default y `--plan`, stderr limpio, nombres duplicados truncados igual que antes.
- **Riesgo:** whitespace, alineación o texto rompe scripts.
- **Aceptación:** salida byte-for-byte salvo tiempo normalizado; ningún cálculo estadístico vive en el renderer.

### Commit 5 — Añadir métricas pasivas de bajo coste

- **Objetivo:** vacíos, min/max, coverage, estado de cardinalidad y diagnostics de nombres, sin afectar planning.
- **Archivos probables:** `src/analysis/model.rs`, `src/analysis/accumulator.rs`, `src/planning/mod.rs`.
- **Pruebas:** unidades de bytes, vacíos quoted/unquoted documentados, censura, overflow saturado y ancho.
- **Riesgo:** memoria por columna y cambio accidental de `recommend_strategy`.
- **Aceptación:** overhead fijo documentado y medido por columna con una regresión/microbenchmark de recursos; `CompressionPlan` idéntico en tests diferenciales; no se activan límites nuevos por defecto.

### Commit 6 — Extraer el scanner delimitado byte-oriented

- **Objetivo:** mover parsing simple/RFC-aware a infraestructura común parametrizada por delimiter.
- **Archivos probables:** nuevo `src/formats/delimited/mod.rs`, `src/formats/mod.rs`, `src/formats/csv/columnar.rs`.
- **Pruebas:** todos los round-trips DCSV01 existentes; multiline, escaped quotes, LF/CRLF, mixed/bare CR, fronteras de chunk y fuzz.
- **Riesgo:** máximo; cualquier cambio de eventos/rangos del adaptador full-slice altera bytes reconstruidos o aceptación. Un campo que cruza buffers no puede prometer un slice contiguo sin copiar/retener el record.
- **Aceptación:** payload/restauración idénticos para corpus soportado; ningún panic; estado de quote cruza buffers.

### Commit 7 — Habilitar delimitadores opt-in solo en `analyze`

- **Objetivo:** TSV, PSV, punto y coma y override de un byte mediante el engine común.
- **Archivos probables:** `src/analysis/engine.rs`, `src/formats/delimited/mod.rs`, `src/cli/mod.rs`.
- **Pruebas:** TSV/PSV, delimitadores en quotes, ambiguous, header modes, TXT con override.
- **Riesgo:** aceptar archivos antes rechazados puede confundir scripts si cambia el default.
- **Aceptación:** el camino legacy por defecto no cambia; nuevos formatos requieren flag opt-in en esta fase; `compress` no cambia.

### Commit 8 — Añadir detección y confianza estructurada

- **Objetivo:** autodetección quote-aware y confidence con razones.
- **Archivos probables:** `src/analysis/engine.rs`, `src/analysis/model.rs`, `src/formats/delimited/mod.rs`.
- **Pruebas:** empates, pocos records, distribución cambiante, records inconsistentes y cobertura completa/parcial.
- **Riesgo:** falsa precisión o defaults inestables.
- **Aceptación:** resultados deterministas, razones visibles, ambigüedad no se convierte en modo de compresión automáticamente.

### Commit 9 — Añadir `analyze --json` versionado

- **Objetivo:** reporte machine-readable sin contaminar el modelo wire.
- **Archivos probables:** nuevo `src/analysis/report.rs` o `src/cli/analysis_report.rs`, `src/cli/mod.rs`, `Cargo.toml` solo si se requiere soporte adicional.
- **Pruebas:** parse con `serde_json`, schema_version, nullability, escaping UTF-8/nombres y stdout/stderr.
- **Riesgo:** convertir accidentalmente el primer JSON en contrato eterno sin versión.
- **Aceptación:** `AnalysisReportV1`, salida determinista y texto legacy intacto.

### Commit 10 — Publicar API Rust experimental y documentación

- **Objetivo:** exponer opciones/resultados después de estabilizar semántica.
- **Archivos probables:** `src/analysis/mod.rs`, `src/lib.rs`, documentación rustdoc, `README.md`.
- **Pruebas:** doctests, consumidores externos de ejemplo y semver surface.
- **Riesgo:** estabilizar demasiado pronto tipos o errores.
- **Aceptación:** visibilidad revisada, `#[non_exhaustive]` donde corresponda y deprecación documentada de `analyze_bytes` sin eliminarla.

### Commit 11 — Alinear modelo de costes y límites del encoder

- **Objetivo:** resolver la brecha entre `ColumnPlan` y `choose_column_mode`.
- **Archivos probables:** `src/planning/mod.rs`, `src/planning/plan.rs`, `src/formats/csv/columnar.rs`, `src/cli/mod.rs`.
- **Pruebas:** selección final por columna, límites efectivos, memoria, fallback y tamaño de archive.
- **Riesgo:** puede cambiar bytes/tamaño de archives v1 nuevos y el camino de memoria; requiere RFC propio o sección explícita de compatibilidad conductual.
- **Aceptación:** una sola función de costes, límites efectivos antes de asignar y reporte distingue estimación de selección final.

### Commit 12 — Benchmarks, property tests y soak de memoria

- **Objetivo:** demostrar coste y cota antes de cambiar defaults.
- **Archivos probables:** ampliar `benches/`, nuevo fuzz target de análisis y tests ignorados/programados de archivos grandes.
- **Pruebas:** repetitivo, alta cardinalidad, ancho, multiline, 1/64/256 MiB, inferencia on/off y multi-GB sintético/sparse.
- **Riesgo:** benchmarks ruidosos o dependientes de plataforma.
- **Aceptación:** baseline guardado, presupuestos verificables, sin regresión significativa acordada de throughput/RSS.

Los commits 1 a 5 forman el alcance recomendado de RFC-001B. Los commits 6 en adelante deben dividirse en RFCs posteriores si el diff o la semántica crecen.

## 12. Matriz de pruebas futura

| Caso | Unit | Integración | CLI golden | Property/fuzz | Benchmark/memoria | Criterio principal |
|---|---:|---:|---:|---:|---:|---|
| CSV simple | Sí | Sí | Sí | Sí | Sí | Mismo plan legacy y métricas deterministas. |
| Comas citadas | Sí | Sí | Sí | Sí | Sí | Delimitadores quoted no crean columnas; round-trip byte-exacto. |
| Newlines citados RFC-aware | Sí | Sí | Sí | Sí | Sí | Un record lógico puede cruzar líneas/buffers y cuenta una sola fila. |
| LF | Sí | Sí | Sí | Sí | No | Detectado como LF; final newline preservado. |
| CRLF | Sí | Sí | Sí | Sí | No | Detectado como CRLF; bytes de campos/newline preservados. |
| Mixed LF/CRLF y bare CR | Sí | Sí | Sí | Sí | No | Resultado/error/diagnostic explícito y determinista. |
| UTF-8 | Sí | Sí | Sí | Sí | No | Longitudes en bytes; corte de chunk/code point sin falso rechazo. |
| UTF-8 inválido | Sí | Sí | Sí | Sí | No | Sin lossy silencioso ni panic; policy documentada. |
| TSV | Sí | Sí | Sí | Sí | Sí | Tab outside quotes, misma infraestructura delimitada. |
| Pipe-delimited | Sí | Sí | Sí | Sí | Sí | Pipe quoted no sesga detección; override funciona. |
| Punto y coma | Sí | Sí | Sí | Sí | No | Se conserva candidato ya reconocido por detector/codec. |
| Delimitador configurable | Sí | Sí | Sí | Sí | No | Override prevalece y valida byte permitido. |
| TXT delimitado | Sí | Sí | Sí | Sí | No | Extensión no fuerza `PlainText` cuando hay override explícito. |
| Sin header | Sí | Sí | Sí | Sí | No | Primera fila se analiza como datos y nombres ordinales estables. |
| Header duplicado | Sí | Sí | Sí | Sí | No | Perfiles separados por índice; JSON no usa nombre como key. |
| Header vacío/inválido | Sí | Sí | Sí | Sí | No | Diagnostic y display name; no se fusionan columnas. |
| Una columna | Sí | Sí | Sí | Sí | Sí | Resultado explícito sin panic; legacy rejection congelado hasta decisión. |
| Muchas columnas | Sí | Sí | No | Sí | Sí | `max_columns`/budget global; error o parcial acotado. |
| Archivo vacío | Sí | Sí | Sí | Sí | No | Error/tipo/exit code estable por modo. |
| Records inconsistentes | Sí | Sí | Sí | Sí | No | Dentro de scope: invalid/conservative; fuera: no reclamar validación completa. |
| Alta cardinalidad | Sí | Sí | Sí | Sí | Sí | `Exact`/`AtLeast`, límite global y memoria acotada. |
| Sampling por filas | Sí | Sí | Sí | Sí | Sí | Detención exacta en límite de record; coverage correcto. |
| Sampling por bytes | Sí | Sí | Sí | Sí | Sí | No overshoot de memoria; bytes leídos/analizados diferenciados. |
| Cambio de distribución tras sample | Sí | Sí | No | Sí | Sí | Confidence baja/partial; planner v1 conserva resultado caracterizado. |
| Delimitador ambiguo | Sí | Sí | Sí | Sí | No | Candidatos/razones; no selección silenciosa para compresión. |
| Record/header gigante | Sí | Sí | Sí | Sí | Sí | Límite duro y error limpio; no asignación del tamaño completo. |
| Archivo lógico de varios GB | No | Sí con reader sintético | No | Sí | Sí/soak | No cargar completo; contador `u64`; RSS dentro de presupuesto. |
| API legacy `analyze_bytes` | Sí | Sí como consumer externo | No | No | No | Firma y metadata CSV/TXT/Unknown no cambian hasta deprecación explícita. |
| v1 RawZstd histórico | No | Sí | No | Mutación/fuzz | No | Restaura bytes y SHA esperados sin regenerar fixture. |
| v1 columnar histórico | No | Sí | No | Mutación/fuzz | No | DCSV01/metadata rica siguen legibles. |
| v2 histórico multichunk | No | Sí | No | Fixture como seed + mutación | No | Tabla, hashes y restore compatibles; el fuzz actual generado con el encoder presente no sustituye el fixture. |

### 12.1 Unit tests

- Acumuladores: vacíos, min/max/media, overflow, cardinalidad exacta/censurada.
- Parser: quotes, escape, delimitador, newline y fronteras arbitrarias de buffers.
- Detección: score, empates, estabilidad y header.
- `PlannerPolicyV1`: goldens estructurales sin timing.
- Memoria lógica: budgets de columnas, hashes y record.

### 12.2 Integration tests

- Un test de consistencia debe probar que `analyze`, `compress` v1 y `benchmark --estimate-only` reciben el mismo modo y sample para opciones equivalentes.
- `compress --chunked` debe continuar produciendo v2 RawZstd sin quedar bloqueado por un análisis delimitado.
- Deben verificarse `ArchiveMode`, `PayloadKind`, versión y fallback, no solo round-trip.
- Los fixtures históricos deben vivir fuera del código generador y tener hashes declarados.

### 12.3 CLI snapshots o goldens

No hay infraestructura de snapshots actual. Se pueden usar fixtures de texto y comparación manual para evitar añadir una dependencia en el primer commit.

Goldens mínimos:

- `analyze` default;
- `analyze --plan` normalizando exclusivamente tiempo;
- archivo ausente, vacío, binario y ancho inconsistente con stderr/exit code;
- futura salida JSON validada además con `serde_json`.

### 12.4 Property tests y fuzzing

El repositorio tiene fuzz targets para round-trip CSV adaptativo y parsing/hardening v2, pero no para el analizador delimitado. El nuevo target debe comprobar:

- nunca panic;
- mismas métricas ante distintas fronteras de lectura;
- bytes/records RFC válidos conservan fronteras lógicas;
- contadores no superan budgets;
- delimitadores quoted no afectan detección;
- implementación extraída y legacy producen el mismo `PlannerPolicyV1` sobre corpus generado.

### 12.5 Benchmarks

`benches/size_placeholder.rs` solo mide zstd. Debe complementarse, no usarse como evidencia del engine.

Mediciones mínimas:

- throughput y allocations para 1, 64 y 256 MiB;
- repetitivo, realista, alta cardinalidad, ancho y multiline;
- inferencia de tipos activada/desactivada;
- bytes inspeccionados, records, columnas y cardinalidad censurada;
- RSS o working set cuando la plataforma lo permita;
- un reader sintético >4 GiB para corrección y un soak sparse/programado para memoria real.

## 13. Cobertura actual y huecos

El conteo estático de la revisión auditada encuentra 173 atributos `#[test]`: 69 en `tests/security_hardening.rs`, 31 en `tests/round_trip.rs` y 73 unitarios bajo `src`. El número 172 se conserva como baseline declarado al cierre de Phase 6, pero el resultado real de `cargo test` es la autoridad para esta revisión.

Cobertura existente útil:

- perfiles repetitivo/random/alta cardinalidad y límites declarativos en `src/planning/tests.rs`;
- delimiter/newline/header simplificados en `src/formats/csv/mod.rs`;
- quoted comma, quotes escapados, vacíos, LF, UTF-8, espacios y hardening DCSV01 en `src/formats/csv/columnar.rs`;
- round-trip, benchmark, v1/v2 y backend chunked/MT en `tests/round_trip.rs`;
- corrupción, límites y atomicidad en `tests/security_hardening.rs`;
- cuatro fuzz targets bajo `fuzz/fuzz_targets/`.

Huecos prioritarios:

- ninguna prueba invoca el subcomando `analyze`;
- no hay goldens de `print_planning_analysis`;
- no hay fixtures `.dpack` históricos;
- `analysis::analyze_bytes` es público, full-buffer y no tiene tests de caracterización;
- no hay test diferencial entre los tres parsers;
- no hay TSV/PSV end-to-end;
- no hay record con newline quoted en el planner;
- no hay límite duro probado para record/header gigante;
- no hay archivo extremadamente ancho ni multi-GB/RSS;
- no hay test que pruebe que los límites por columna gobiernan el encoder, porque hoy no lo hacen.

## 14. Decisiones pendientes

1. ¿El API público mantiene defaults planner-compatible o nace con auto-detección mientras el CLI legacy conserva compatibilidad?
2. ¿Qué política se aplica a UTF-8 inválido: análisis de bytes, error o nombres omitidos?
3. ¿`HeaderMode::Auto` puede ser default en algún comando sin romper scripts?
4. ¿Una columna se considera delimitada, plain text o unsupported?
5. ¿Mixed newline es error, warning o formato detectado?
6. ¿La cardinalidad aproximada seguirá con hashes capped o se justifica HyperLogLog/otro sketch?
7. ¿Cuál es el presupuesto global por defecto para columnas, cardinalidad y tamaño de record?
8. ¿Confidence será solo cualitativa o incluirá un score calibrado?
9. ¿La salida JSON se versiona por comando, crate o schema independiente?
10. ¿`CompressionPlan` será un plan ejecutable o una recomendación? Esta decisión determina cómo resolver `choose_column_mode` y los límites.
11. ¿El análisis de TSV/PSV será inicialmente solo explícito o también auto-detectado?
12. ¿`benchmark --max-input-mb` debe alinear el scope del planner en un cambio futuro, aceptando que puede variar `estimated_mode`?
13. ¿Cómo se obtendrán y versionarán fixtures históricos reales de v1/v2?

## 15. Quality gates de esta auditoría

Los resultados se registran después de crear únicamente este documento:

| Comando | Resultado real |
|---|---|
| `cargo fmt --check` | **Falló** (exit 1). Rustfmt pide partir la cadena `archive.pop().expect(...)` en `tests/security_hardening.rs:544`. Es una diferencia preexistente y no se modificó por ser código fuera del alcance documental. |
| `cargo check` | **Pasó** (exit 0). Terminó el perfil `dev` sin errores. |
| `cargo test` | **Pasó** (exit 0). 173 tests: 73 unitarios, 31 en `round_trip`, 69 en `security_hardening`; 0 fallos, 0 ignorados. Los targets sin tests y doctests también terminaron correctamente. |
| `cargo clippy --all-targets --all-features -- -D warnings` | **Pasó** (exit 0). Sin warnings bajo `-D warnings`. |

No se contará la ejecución puntual de `cargo run` como quality gate.

## 16. Archivos inspeccionados

Código y manifiestos:

- `Cargo.toml`
- `src/main.rs`
- `src/lib.rs`
- `src/cli/mod.rs`
- `src/analysis/mod.rs`
- `src/planning/mod.rs`
- `src/planning/plan.rs`
- `src/planning/tests.rs`
- `src/formats/mod.rs`
- `src/formats/csv/mod.rs`
- `src/formats/csv/columnar.rs`
- `src/formats/txt/mod.rs`
- `src/metadata/mod.rs`
- `src/storage/mod.rs`
- `src/storage/chunked.rs`
- `src/storage/output.rs`
- `src/compression/mod.rs`
- `src/compression/zstd_backend/mod.rs`
- `src/error/mod.rs`

Pruebas, fuzz y benchmarks:

- `tests/round_trip.rs`
- `tests/security_hardening.rs`
- `benches/size_placeholder.rs`
- `fuzz/fuzz_targets/csv_roundtrip.rs`
- `fuzz/fuzz_targets/v2_archive_parser.rs`
- `fuzz/fuzz_targets/v2_decompress_mutated_archive.rs`
- `fuzz/fuzz_targets/chunk_table_validation.rs`

Documentación y gates:

- `README.md`
- `ARCHITECTURE.md`
- `SECURITY.md` (referencias de Phase 6)
- `scripts/check.ps1`
- `scripts/check.sh`

## 17. Recomendación Go / No-Go

### Go condicionado

Proceder con RFC-001B únicamente para:

1. fixtures/goldens y tests de caracterización;
2. extracción de un modelo factual `pub(crate)` independiente de `DpackMetadata`;
3. facade compartido por `analyze`, `compress` v1 y `benchmark`;
4. encapsulación de reglas actuales como `PlannerPolicyV1` sin cambiar umbrales;
5. separación del renderer legacy;
6. métricas pasivas de coste bajo sin influencia en selección;
7. instrumentación, diagnostics y regresiones de recursos que permitan diseñar presupuestos duros en el RFC posterior del scanner, sin activarlos todavía por defecto.

### No-Go en RFC-001B

No proceder todavía con:

- sustitución del parser legacy del planner por el RFC-aware;
- publicación de una API Rust declarada estable;
- `analyze --json` como contrato no versionado;
- activación por defecto de autodetección/header inference;
- TSV/PSV columnar en `compress`;
- cambios a `DpackMetadata`, `FileType`, v1, DCSV01 o v2;
- cambios de `build_plan`, umbrales o `choose_column_mode`;
- entropía por columna, fechas/timestamps o trial zstd por defecto.

### Riesgos bloqueantes para fases posteriores

- ausencia de fixtures históricos v1/v2;
- ausencia de goldens del CLI y del planner;
- límite de sample no duro para records/headers gigantes;
- presupuesto de cardinalidad por columna, no global;
- tres parsers con semánticas distintas;
- `ColumnPlan` y límites desconectados del encoder;
- v1 columnar whole-file para entradas grandes;
- ambigüedad de header/delimiter y confianza no modelada.

### Recomendación concreta para RFC-001B

Titular RFC-001B como **“Extracción compatible del núcleo de análisis y PlannerPolicyV1”**. Su criterio de aceptación debe ser: mismos planes, mismos modos finales, mismos archives deterministas en la misma build, misma salida CLI legacy, mismos exit codes, sin cambios v1/v2 y sin regresión de memoria; además, un nuevo modelo factual interno y pruebas que demuestren que `analyze`, `compress` v1 y `benchmark` consumen la misma instancia semántica.

La generalización RFC-aware y multi-delimiter debe comenzar solo después, sobre el scanner común y primero como opción de análisis. Esa secuencia convierte el Intelligence Engine en una evolución verificable del planner existente, no en una segunda fuente de verdad.
