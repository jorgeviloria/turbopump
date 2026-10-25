# Anillos de desgaste y álabes de rebombeo — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Objetivo:** el contorno meridional trae sellos delantero y trasero con pares de anillos
postizos (rodete y carcasa) sobre asientos, editables y desactivables, y álabes de
rebombeo opcionales en la cara trasera del disco del cubo.

**Arquitectura:** `impeller/seals.rs` es la única fuente de los sellos. `material.rs`
dibuja los asientos del rodete, `chambers.rs` los de la carcasa, `tp-cad` revoluciona los
anillos como sólidos aparte y `tp-project` pasa d_sp, d_s2 y s al rendimiento.
`impeller/back_vanes.rs` modela los álabes de rebombeo y los consumen el meridiano, la
cámara trasera, la CAD y el empuje axial.

**Tecnología:** Rust (workspace), serde, OCCT vía `tp-cad`/`tp-occt-sys`, egui 0.36 y
kittest.

**Especificación:** `docs/superpowers/specs/2026-10-24-anillos-y-rebombeo-design.md`

## Restricciones globales

- Código y comentarios en inglés; textos visibles en inglés con su traducción en
  `crates/tp-gui/src/i18n/es.rs` (`tr`/`tf`).
- Valores automáticos con `Param<T>` (`Param::Auto` / `Param::Manual`), resueltos con
  `Param::resolve(|| auto)`.
- Por defecto en diseños nuevos: los dos sellos activos, con los cuatro anillos activos.
  Los álabes de rebombeo vienen desactivados.
- Valores automáticos:
  - delantero: d_sp = d₁ + 2t;
  - trasero: d_s2 = d_sp;
  - L = `SEAL_LENGTH_RATIO`·d (0.12, Gülich 3.6.2);
  - s = `losses::seal_clearance(d)` (Gülich ec. 3.12);
  - e_r = e_c = máx(0.03·d, 4 mm), elección nuestra.
- Los proyectos antiguos abren con el sello delantero según su `neck_length` (0 lo
  desactiva), y con el sello trasero y los anillos postizos desactivados.
- Los avisos nunca bloquean el diseño: van a `material_warnings` / `chambers.warnings`.
- Unidades fuera de los campos, nada se mueve al pasar el ratón o al seleccionar, y
  pruebas de estabilidad en la GUI.
- No se fija ningún número sin fuente verificada en el PDF (tarea 10). Gülich: página
  del PDF = página del libro + 35. Lobanoff: + 15.
- Antes de cada commit, compilar sin las características por defecto:
  `cargo build -p tp-cli --no-default-features`, `cargo build -p tp-gui
  --no-default-features` y `cargo build -p tp-fluids --no-default-features`.
- Commits con el skill `commit` (identidad de Jorge, gitmoji, en español), siempre con
  rutas explícitas. El árbol puede estar compartido con otra sesión.

## Puntos a vigilar en la revisión

1. **Rodetes pequeños:** con d_sp pequeño, e_r mínimo de 4 mm puede dejar el asiento por
   debajo del ojo. Debe salir un aviso y recortarse e_r para que quede la pared t, sin
   pánico. Prueba `a_ring_too_thick_for_the_eye_is_trimmed_with_a_warning` (tarea 1).
2. **Hilo de material dibujado a mano:** un hilo dibujado no lleva asiento. Los anillos
   de ese lado no se construyen en la CAD y sale un aviso. Prueba
   `a_drawn_wire_leaves_its_rings_out_with_a_warning` (tarea 6).
3. **Rodete de doble entrada:** el sello trasero no aplica. Se ignora con aviso y se
   oculta en la GUI. Prueba `a_double_entry_impeller_has_no_rear_seal` (tarea 1).
4. **Semiaxial con d₂i < d_s2:** el collar trasero quedaría fuera del disco. Se recorta d_s2
   a d₂i − 2t con aviso. Prueba `a_rear_seal_beyond_the_disk_rim_is_trimmed` (tarea 4).
5. **Desactivar y reactivar** un sello o anillo conserva sus valores a mano. Prueba
   `toggling_a_seal_keeps_its_manual_values` (tarea 8).

---

### Tarea 1: modelo de sellos `seals.rs`

**Archivos:**
- Crear: `crates/tp-design/src/impeller/seals.rs`
- Modificar: `crates/tp-design/src/impeller/mod.rs` (declarar `pub mod seals;`)

**Interfaces:**
- Produce:
  ```rust
  pub const RING_THICKNESS_RATIO: f64 = 0.03;
  pub const MIN_RING_THICKNESS: f64 = 0.004;
  pub fn default_ring_thickness(d: f64) -> f64; // máx(0.03·d, 4 mm)

  #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] #[serde(default)]
  pub struct RingSettings { pub enabled: bool, pub thickness: Param<f64> }
  // Default: enabled = true, thickness = Auto

  #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] #[serde(default)]
  pub struct SealSettings {
      pub enabled: bool, pub diameter: Param<f64>, pub length: Param<f64>,
      pub clearance: Param<f64>,
      pub impeller_ring: RingSettings, pub casing_ring: RingSettings,
  }
  #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] #[serde(default)]
  pub struct Seals { pub front: SealSettings, pub rear: SealSettings }
  impl Seals {
      /// Sellos de un proyecto anterior: delantero según `neck_length` (Manual(0) lo
      /// apaga), sin trasero y sin anillos postizos.
      pub fn legacy(neck_length: Param<f64>) -> Self;
      /// `eyes`: 1 o 2 (`ImpellerBasis` no lo lleva; viene del diseño principal).
      pub fn resolve(&self, basis: &ImpellerBasis, wall_thickness: f64, eyes: f64)
          -> ResolvedSeals;
  }

  /// Radios en m.
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Seal {
      pub radius: f64,              // r: exterior del anillo del rodete, en la holgura
      pub length: f64, pub clearance: f64,
      pub impeller_seat: f64,       // r − e_r, o r sin anillo
      pub casing_seat: f64,         // r + s + e_c, o r + s sin anillo
      pub impeller_ring: Option<f64>, // e_r
      pub casing_ring: Option<f64>,   // e_c
  }
  #[derive(Debug, Clone, PartialEq)]
  pub struct ResolvedSeals {
      pub front: Option<Seal>, pub rear: Option<Seal>, pub warnings: Vec<String>,
  }
  ```

- [ ] **Paso 1: escribir las pruebas que fallan** (en `seals.rs`, `mod tests`, con
  `crate::impeller::meridian::tests::basis()`):
  - `default_seals_have_both_pairs_of_rings`: con `Seals::default().resolve(&b, t)` los
    dos sellos son `Some`. `front.radius == (b.inlet_diameter + 2t)/2`,
    `rear.radius == front.radius`, `length == 0.12·2r`,
    `clearance == seal_clearance(2r)`, `impeller_seat == r − e`,
    `casing_seat == r + s + e`, con `e == default_ring_thickness(2r)`.
  - `a_disabled_ring_leaves_the_part_solid_to_the_gap`: con `impeller_ring.enabled =
    false`, `impeller_seat == radius`. Con `casing_ring.enabled = false`,
    `casing_seat == radius + clearance`.
  - `a_disabled_seal_is_none`.
  - `legacy_seals_keep_the_old_front_neck`: `Seals::legacy(Param::Manual(0.02))` da el
    delantero con `length == 0.02` y sin anillos, y `rear == None`.
    `Seals::legacy(Param::Manual(0.0))` da el delantero `None`.
  - `a_ring_too_thick_for_the_eye_is_trimmed_with_a_warning`: con e_r = `Manual(0.5)`,
    `impeller_seat >= b.inlet_diameter/2 + t` y `warnings` no vacío.
  - `a_double_entry_impeller_has_no_rear_seal`: con `eyes = 2.0`, `rear == None` y hay
    un aviso. Para llegar a `MeridionalContour::build`, `MeridionalInput` no sirve: la
    tarea 2 añade `eyes` a la llamada desde `tp-project`, que lo toma de
    `CentrifugalPumpDesign::eyes`.
  - `a_clearance_below_guelich_minimum_warns`: con s = `Manual(1e-5)`, hay un aviso.
- [ ] **Paso 2:** `cargo test -p tp-design seals`. Debe fallar porque el módulo no existe.
- [ ] **Paso 3: implementar.** `resolve` toma los automáticos de la tabla de las
  restricciones globales. Recorta e_r para que `impeller_seat - basis.inlet_diameter/2 >=
  wall_thickness`, con aviso. Avisa si `clearance < seal_clearance(2r)`.
- [ ] **Paso 4:** `cargo test -p tp-design seals`. Debe pasar.
- [ ] **Paso 5:** commit con `seals.rs` y `mod.rs`: `✨ feat(design): modelo de sellos con
  pares de anillos postizos`.

### Tarea 2: sellos en `MeridionalInput` y migración del formato

**Archivos:**
- Modificar: `crates/tp-design/src/impeller/meridian.rs` (`MeridionalInput`,
  `MeridionalContour`)
- Modificar: `crates/tp-design/src/impeller/material.rs` (quitar `neck_length` de
  `MaterialSettings` y de `Material`; el cuello toma `seals.front`)
- Modificar: `crates/tp-project/src/file.rs` (`FORMAT_VERSION` 1 → 2, migración)
- Modificar: `crates/tp-gui/src/steps.rs:2300-2310` (quitar la fila «Neck length»; la
  tarjeta llega en la tarea 8)
- Modificar: `crates/tp-design/src/impeller/chambers.rs` (prueba de la línea 292:
  compara con `seals.front.length`)

**Interfaces:**
- Consume: `Seals`, `ResolvedSeals` (tarea 1).
- Produce:
  - `MeridionalInput::seals: Seals`, con `Seals::default()` en `MeridionalInput::default()`;
  - `MeridionalContour::seals: ResolvedSeals`;
  - `material::automatic_shroud(hub, shroud, m, front: Option<&Seal>)`;
  - `material::automatic_hub(hub, shroud, basis, m, rear: Option<&Seal>)`;
  - `material::wires(..., seals: &ResolvedSeals)`;
  - en `file.rs`, `fn migrate_v1_seals(project: &mut serde_json::Value)`.

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - En `file.rs`, `a_version_1_file_gets_legacy_seals`: un JSON de versión 1 con
    `meridional.material.neck_length = 0.015` carga con `seals.front.length ==
    Manual(0.015)`, `seals.rear.enabled == false` y los anillos desactivados.
  - `a_version_1_file_without_a_neck_keeps_the_automatic_one`: sin `neck_length`, el
    sello delantero queda activo con largo `Auto` y sin anillos.
  - En `material.rs`, la prueba existente
    `the_automatic_shroud_is_the_shroud_moved_outwards` compara el largo del cuello con
    `c.seals.front.unwrap().length`.
- [ ] **Paso 2:** `cargo test -p tp-project file` y `cargo test -p tp-design material`.
  Deben fallar.
- [ ] **Paso 3: implementar.**
  - `migrate_v1_seals` recorre el JSON. En cada objeto que tenga
    `meridional.material`, saca `neck_length` (si existe), escribe
    `meridional.seals = serde_json::to_value(Seals::legacy(neck))`, mueve
    `meridional.chambers.seal_clearance` (si existe) a `meridional.seals.front.clearance`
    y se llama desde `from_json` cuando `header.version < 2`.
  - `MeridionalContour::build(basis, input)` pasa a `build(basis, input, eyes: f64)`.
    Hay que actualizar sus llamadas en `tp-project`, `tp-gui` y las pruebas; en las
    pruebas, `1.0`.
  - `MeridionalContour::build` resuelve los sellos antes de `material::wires`.
  - El cuello delantero usa `front.map_or(0.0, |s| s.length)` hasta la tarea 3.
- [ ] **Paso 4:** `cargo test -p tp-design -p tp-project` y las tres compilaciones sin
  características por defecto. Debe pasar.
- [ ] **Paso 5:** commit: `🗃️ feat(project): los sellos entran en el meridiano y el
  formato pasa a la versión 2`.

### Tarea 3: asiento y tope del sello delantero en el sudario

**Archivos:**
- Modificar: `crates/tp-design/src/impeller/material.rs` (`automatic_shroud`)

**Interfaces:**
- Consume: `Seal` (tarea 1).
- Produce: con sello delantero, el hilo del sudario recorre el borde a la salida, la cara
  exterior desplazada, el tope a `r = seal.radius`, el asiento a `r = impeller_seat` a lo
  largo de `length` y el ojo. Sin sello, recorre la cara exterior hasta el ojo, sin cuello.

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - `the_front_seat_runs_on_the_seat_radius`: hay un segmento `Line` con los dos
    extremos a `y == front.impeller_seat` y largo `front.length`.
  - `the_front_shoulder_reaches_the_seal_radius`: algún punto del hilo está a
    `y == front.radius` en la x de inicio del asiento.
  - `without_a_front_seal_the_shroud_has_no_neck`: el punto más aguas arriba del hilo
    es el inicio del sudario.
  - `a_low_shoulder_warns`: si la cara exterior del sudario no llega a `front.radius`,
    hay un aviso en `material_warnings`.
- [ ] **Paso 2:** `cargo test -p tp-design material`. Debe fallar.
- [ ] **Paso 3: implementar.** El tope es un segmento radial desde la cara exterior hasta
  `radius` si la cara queda por encima de él. Si queda por debajo, se avisa y el tope se
  hace en la cara.
- [ ] **Paso 4:** `cargo test -p tp-design`. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(design): el sudario lleva el asiento del anillo
  delantero`.

### Tarea 4: collar y asiento del sello trasero en el disco del cubo

**Archivos:**
- Modificar: `crates/tp-design/src/impeller/material.rs` (`automatic_hub`)
- Modificar: `crates/tp-cad/src/impeller.rs:510-530` (`hole_pattern`: el diámetro primitivo
  queda entre el saliente y el interior del collar, `2·(impeller_seat − t)`)

**Interfaces:**
- Consume: `Seal` (tarea 1).
- Produce:
  - el hilo del disco baja por la cara trasera hasta `impeller_seat`;
  - sale hacia atrás `length` a ese radio, baja `t` (el espesor del collar), vuelve a la
    cara trasera y sigue hasta el saliente;
  - `fn collar_inner_radius(rear: &Seal, t: f64) -> f64` (pública en `material.rs`), que
    usan la CAD y las cámaras.

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - `the_rear_collar_stands_behind_the_disk`: hay un segmento `Line` a
    `y == rear.impeller_seat` de largo `rear.length`, y su x final está detrás de la cara
    trasera.
  - `the_collar_wall_is_the_disk_thickness`.
  - `a_collar_on_the_boss_warns`: si `collar_inner_radius <= boss/2`, hay un aviso.
  - `a_rear_seal_beyond_the_disk_rim_is_trimmed`: con d_s2 = `Manual(d₂)`, el radio usado
    es menor o igual que d₂i/2 − t y hay un aviso.
  - En `tp-cad`, `balance_holes_sit_inside_the_collar`: el diámetro primitivo queda entre
    el saliente y `2·collar_inner_radius`.
- [ ] **Paso 2:** `cargo test -p tp-design material` y `cargo test -p tp-cad
  balance_holes`. Deben fallar.
- [ ] **Paso 3: implementar.** La cara trasera puede ser curva: el collar sale desde el
  punto de la cara a `impeller_seat`, que se halla por interpolación como en el corte
  actual con el saliente.
- [ ] **Paso 4:** `cargo test -p tp-design -p tp-cad`. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(design): el disco del cubo lleva el collar del anillo
  trasero`.

### Tarea 5: asientos de la carcasa en las cámaras laterales

**Archivos:**
- Modificar: `crates/tp-design/src/impeller/chambers.rs` (`build`, `ChamberValues`)
- Modificar: `crates/tp-design/src/impeller/meridian.rs:622` (pasa `&ResolvedSeals`)

**Interfaces:**
- Consume: `ResolvedSeals` (tarea 2) y `collar_inner_radius` (tarea 4).
- Produce:
  - `chambers::build(cover, disk, inside, basis, settings, seals: &ResolvedSeals)`;
  - `ChamberValues` toma `seal_diameter`, `seal_clearance` y `seal_length` del sello
    delantero (ya no los infiere del cuello);
  - nuevos campos `rear_seal_diameter: Option<f64>` y `rear_seal_clearance: Option<f64>`;
  - `ChamberSettings::seal_clearance` se elimina: la holgura vive en `SealSettings`. La
    migración de la tarea 2 mueve un valor manual a `seals.front.clearance`.

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - `the_front_casing_runs_on_the_casing_seat`: la pared delantera tiene un tramo a
    `y == front.casing_seat` de largo `front.length`.
  - `the_seal_gap_is_the_clearance`: la distancia radial entre la pared de la carcasa en
    la holgura (`radius + clearance`) y el anillo del rodete (`radius`) es `clearance`.
  - `the_rear_chamber_wraps_the_collar`: el contorno `rear` pasa por la punta del collar
    y por el interior del collar hasta d_D.
  - `without_seals_the_chambers_close_on_the_faces`.
  - Se ajusta la prueba existente de la línea 290.
- [ ] **Paso 2:** `cargo test -p tp-design chambers`. Debe fallar.
- [ ] **Paso 3: implementar.**
  - La pared de la carcasa sigue `casing_seat` a lo largo del anillo. El contorno fluido
    de la holgura va entre `radius` y `radius + clearance`.
  - Atrás, la pared queda a s_ax de la punta del collar y baja como placa hasta d_D.
- [ ] **Paso 4:** `cargo test -p tp-design`, más la migración de `tp-project`. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(design): las cámaras laterales llevan los asientos de
  la carcasa`.

### Tarea 6: los anillos como sólidos en la CAD y la exportación

**Archivos:**
- Modificar: `crates/tp-cad/src/impeller.rs` (`ImpellerCad`, `build_impeller`)
- Modificar: `crates/tp-gui/src/export.rs:410-440` y `crates/tp-cli/src/cad.rs:220-240`
  (archivos y nombres de los anillos, más el informe)

**Interfaces:**
- Consume: `MeridionalContour::seals` (tarea 2).
- Produce:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RingKind { FrontImpeller, FrontCasing, RearImpeller, RearCasing }
  impl RingKind { pub fn file_stem(self) -> &'static str }
  // "front-impeller-ring", "front-casing-ring", "rear-impeller-ring", "rear-casing-ring"
  pub struct WearRing { pub kind: RingKind, pub solid: Shape }
  // ImpellerCad::rings: Vec<WearRing>
  impl ImpellerCad { pub fn ring_volume(&self, kind: RingKind) -> Option<f64> } // m³
  ```
  Cada anillo es la revolución de su rectángulo meridional:
  - delantero del rodete: r de `impeller_seat` a `radius`, desde la punta del cuello
    hasta el tope;
  - carcasa: r de `radius + clearance` a `casing_seat`, en el mismo tramo axial;
  - traseros: igual, sobre el collar.

- [ ] **Paso 1: escribir las pruebas que fallan** (marcadas `#[ignore]` si construyen
  OCCT, igual que las pruebas CAD actuales):
  - `four_rings_are_built_by_default`.
  - `the_ring_volume_is_its_annulus`: `ring_volume ≈ π·(r_o² − r_i²)·L`, con 1e-6 de
    tolerancia relativa.
  - `rings_do_not_overlap_the_impeller`: el volumen de `impeller.common(ring)` es menor
    que 1e-9·el volumen del anillo.
  - `impeller_plus_its_rings_fills_the_solid_seat`: el volumen del rodete con asiento
    más el del anillo del rodete es igual al del rodete con los anillos del rodete
    desactivados, con 1e-4 de tolerancia relativa.
  - `a_drawn_wire_leaves_its_rings_out_with_a_warning`.
- [ ] **Paso 2:** `cargo test -p tp-cad -- --ignored rings`. Debe fallar.
- [ ] **Paso 3: implementar.**
  - La CAD no construye el anillo del rodete de un lado si su hilo es dibujado
    (`input.hub_material` o `input.shroud_material` es `Some`), y lo avisa en
    `warnings`. Para eso, `MeridionalContour` expone `hub_material_drawn: bool` y
    `shroud_material_drawn: bool`.
  - La exportación escribe un archivo por anillo con `file_stem()`, como hoy se hace con
    `side-chambers`. Siempre son archivos separados: así se cumple lo de «cuerpos con
    nombre» de la especificación, estén las cámaras activas o no. El informe da el
    volumen y la masa de cada uno.
- [ ] **Paso 4:** `cargo test -p tp-cad -- --ignored rings`, `cargo test -p tp-cli` y las
  compilaciones sin características por defecto. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(cad): los anillos de desgaste se exportan como sólidos
  aparte`.

### Tarea 7: el rendimiento y el empuje axial leen los sellos

**Archivos:**
- Modificar: `crates/tp-project/src/design.rs:245-275` (renombrar `with_chamber_seals` a
  `with_contour_seals`)
- Modificar: `crates/tp-gui/src/views/performance.rs:30,82,239` (la casilla de agujeros
  de equilibrio refleja el sello trasero; si está apagado, queda deshabilitada con su
  explicación)

**Interfaces:**
- Consume: `MeridionalContour::seals` (tarea 2) y `ChamberValues` (tarea 5).
- Produce: `Project::with_contour_seals(id, input) -> PerformanceInput`, que llena donde
  `input` está en `Auto`:
  - `axial.seal_diameter = 2·front.radius`;
  - `losses.seal_clearance = wear·front.clearance`;
  - `losses.seal_length = front.length`;
  - `axial.rear_seal_diameter = 2·rear.radius`.

  Lo hace siempre que exista el sello, haya cámaras o no. El hueco axial y d_D siguen
  saliendo de las cámaras cuando están activas.

- [ ] **Paso 1: escribir las pruebas que fallan** (en `design.rs`):
  - `performance_takes_the_front_seal_without_chambers`.
  - `performance_takes_the_rear_seal_for_balance_holes`.
  - `manual_performance_values_beat_the_seals`.
  - `automatic_seals_keep_todays_axial_thrust`: con los sellos en automático y las
    cámaras apagadas, `compute_performance(...).axial` es igual al de hoy, con 1e-9 de
    tolerancia relativa. La razón es que d₁ + 2t coincide con el automático de
    `axial_thrust`. Si `default_shroud_thickness` difiere de `default_wall_thickness`, la
    prueba fija la diferencia esperada y se anota en la especificación.
- [ ] **Paso 2:** `cargo test -p tp-project`. Debe fallar.
- [ ] **Paso 3: implementar.** Hay que renombrar todas las llamadas a
  `with_chamber_seals`.
- [ ] **Paso 4:** `cargo test -p tp-project -p tp-gui`. Debe pasar.
- [ ] **Paso 5:** commit: `♻️ refactor(project): el rendimiento toma los sellos del
  meridiano`.

### Tarea 8: tarjetas de sellos en la GUI

**Archivos:**
- Modificar: `crates/tp-gui/src/steps.rs` (paso de material del Meridiano, junto a las
  filas de espesor, eje y saliente)
- Modificar: `crates/tp-gui/src/i18n/es.rs`
- Modificar: `crates/tp-gui/tests/ui.rs` y `crates/tp-gui/tests/screenshots.rs`

**Interfaces:**
- Consume: `MeridionalInput::seals` y `MeridionalContour::seals`, para los valores
  usados.
- Produce:
  - `fn seal_card(ui, title: &str, seal: &mut SealSettings, used: Option<&Seal>, units:
    &DisplayUnits) -> bool` en `steps.rs`;
  - filas de diámetro, largo y holgura con `param_f64`;
  - en cada anillo, una casilla y su espesor;
  - la tarjeta trasera incluye la casilla de agujeros de equilibrio de la tarea 7.

  Textos en inglés:
  - títulos: "Front seal", "Rear seal";
  - filas: "Seal diameter", "Seal length", "Radial clearance";
  - anillos: "Impeller ring", "Casing ring", "Ring thickness";
  - ayudas: "Automatic: d₁ + 2t" / "Automatic: d_sp" / "Automatic: 0.12·d (Gülich
    Chap. 3.6.2)" / "Automatic: Gülich eq. 3.12" / "Automatic: max(0.03·d, 4 mm), our
    choice";
  - traducciones: «Sello delantero», «Sello trasero», «Diámetro del sello», «Largo del
    sello», «Holgura radial», «Anillo del rodete», «Anillo de la carcasa», «Espesor del
    anillo».

- [ ] **Paso 1: escribir las pruebas que fallan** (en `tests/ui.rs`):
  - `the_material_step_shows_both_seal_cards`.
  - `toggling_a_seal_keeps_its_manual_values`: fijar d_sp a mano, apagar el sello,
    encenderlo y comprobar que sigue `Manual` con el mismo valor.
  - `seal_cards_do_not_move_on_hover`: posiciones iguales antes y después de pasar el
    ratón, como en las pruebas de estabilidad existentes.
  - `a_double_entry_impeller_hides_the_rear_seal`.
- [ ] **Paso 2:** `cargo test -p tp-gui --test ui seal`. Debe fallar.
- [ ] **Paso 3: implementar.** Con el sello apagado, sus filas se ocultan pero los
  valores se conservan en `SealSettings`.
- [ ] **Paso 4:** `cargo test -p tp-gui`, regenerar las capturas con `cargo test -p
  tp-gui --test screenshots -- --ignored` y revisarlas. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(gui): tarjetas de los sellos delantero y trasero en el
  paso de material`.

### Tarea 9: claves de sellos en la CLI

**Archivos:**
- Modificar: `crates/tp-cli/src/geometry.rs:350` (el `match key` del meridiano)

**Interfaces:**
- Consume: `MeridionalInput::seals`.
- Produce: claves con guion, según la convención de la CLI. La especificación decía
  `front_seal.*`; se ajusta a la forma `<lado>-seal…`, con `<lado>` = `front` | `rear`:
  - `<lado>-seal` = `on|off`;
  - `<lado>-seal-d`, `<lado>-seal-length`, `<lado>-seal-s` (longitud o `auto`);
  - `<lado>-impeller-ring` = `on|off`, `<lado>-impeller-ring-e`;
  - `<lado>-casing-ring` = `on|off`, `<lado>-casing-ring-e`.

- [ ] **Paso 1: escribir las pruebas que fallan:** `front_seal_keys_set_the_seal`,
  `ring_switches_accept_on_and_off` y `an_unknown_switch_value_is_an_error` (con el
  mensaje `front-seal must be on or off, got "x"`).
- [ ] **Paso 2:** `cargo test -p tp-cli seal`. Debe fallar.
- [ ] **Paso 3: implementar** junto a las claves `axial-length` y similares, y añadirlas a
  la ayuda del comando.
- [ ] **Paso 4:** `cargo test -p tp-cli` y `cargo build -p tp-cli --no-default-features`.
  Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(cli): claves de los sellos y anillos en geometry`.

### Tarea 10: verificar en la literatura los números pendientes

Esta tarea no lleva código: produce las fuentes que necesitan las tareas 11 a 14.

**Archivos:**
- Modificar: `docs/superpowers/specs/2026-10-24-anillos-y-rebombeo-design.md` (sección
  «Fuentes verificadas»)

- [ ] **Paso 1:** en Gülich 2.ª ed., cap. 9.2 (PDF = libro + 35), buscar el efecto de los
  álabes de rebombeo sobre el factor de rotación k y la presión en el disco trasero,
  con la ecuación y su número. Las ecuaciones se leen como imagen.
- [ ] **Paso 2:** en Stepanoff (cap. de empuje axial; el escaneo solo tiene las páginas
  pares) y en Lobanoff & Ross (PDF = libro + 15), buscar el número, la altura y la
  holgura típicos de los álabes de rebombeo, y la potencia que consumen.
- [ ] **Paso 3:** buscar en la literatura disponible un criterio de espesor de anillos
  postizos que confirme o sustituya máx(0.03·d, 4 mm).
- [ ] **Paso 4:** anotar en la especificación cada valor con su autor, ecuación y
  página, y los que no tengan fuente como «elección nuestra». Si e_r cambia, se
  actualizan `RING_THICKNESS_RATIO` y `MIN_RING_THICKNESS`, junto con su prueba.
- [ ] **Paso 5:** commit: `📝 docs: fuentes de los álabes de rebombeo y del espesor de
  anillos`.

### Tarea 11: modelo y meridiano de los álabes de rebombeo

**Archivos:**
- Crear: `crates/tp-design/src/impeller/back_vanes.rs`
- Modificar: `crates/tp-design/src/impeller/mod.rs`,
  `crates/tp-design/src/impeller/meridian.rs` (`MeridionalInput::back_vanes`,
  `MeridionalContour::back_vanes`) y `crates/tp-design/src/impeller/chambers.rs`
  (cámara trasera)

**Interfaces:**
- Consume: los valores de la tarea 10 y `ResolvedSeals` (tarea 2).
- Produce:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
  pub enum VaneShape { Radial, Curved { angle: f64 } } // ángulo en rad
  #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] #[serde(default)]
  pub struct BackVaneSettings {
      pub enabled: bool, pub count: Param<u32>, pub shape: VaneShape,
      pub outer_diameter: Param<f64>, pub inner_diameter: Param<f64>,
      pub height: Param<f64>, pub thickness: Param<f64>, pub tip_clearance: Param<f64>,
  }
  #[derive(Debug, Clone, PartialEq)]
  pub struct BackVanes {
      pub count: u32, pub shape: VaneShape, pub thickness: f64, pub tip_clearance: f64,
      /// Banda meridional cerrada (z, r), m: la cara trasera entre d_int y d_ext y la
      /// misma cara desplazada h hacia atrás.
      pub band: Vec<DVec2>,
      pub inner_radius: f64, pub outer_radius: f64, pub height: f64,
      pub warnings: Vec<String>,
  }
  pub fn build(disk: &Wall, inside: DVec2, settings: &BackVaneSettings,
               basis: &ImpellerBasis, material: &Material, blade_thickness: f64,
               rear: Option<&Seal>) -> Option<BackVanes>; // None si están desactivados
  ```

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - `back_vanes_are_off_by_default`.
  - `the_band_lies_behind_the_disk_at_the_vane_height`: cada punto exterior de la banda
    está a `height` de la cara trasera, con 1 % de tolerancia.
  - `vanes_stop_outside_the_rear_casing_ring_with_a_warning`: con el sello trasero
    activo, `inner_radius >= rear.casing_seat` y hay un aviso.
  - `the_rear_chamber_clears_the_vane_tips`: en la zona de los álabes, la pared trasera
    de la carcasa está a `height + tip_clearance` de la cara.
- [ ] **Paso 2:** `cargo test -p tp-design back_vanes`. Debe fallar.
- [ ] **Paso 3: implementar** con los valores automáticos de la tarea 10. La banda se
  hace con `offset_points` sobre el tramo de la cara trasera entre los dos radios.
- [ ] **Paso 4:** `cargo test -p tp-design` y `cargo test -p tp-project`. El campo
  `back_vanes` es `#[serde(default)]` y los proyectos antiguos lo cargan desactivado.
  Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(design): álabes de rebombeo en la cara trasera del
  disco`.

### Tarea 12: álabes de rebombeo en la CAD

**Archivos:**
- Modificar: `crates/tp-cad/src/impeller.rs` (`build_impeller`)

**Interfaces:**
- Consume: `MeridionalContour::back_vanes` (tarea 11).
- Produce: `ImpellerCad::back_vanes: Option<Shape>`, ya unidos a `impeller`.
  - Los álabes salen de revolucionar `band` y hacer su intersección con `count` prismas
    en planta, del grosor `thickness`.
  - Los radiales son tiras rectas por el eje. Los curvos, espirales logarítmicas
    `θ(r) = ln(r/r_in)/tan(angle)`, en el plano xy, extruidas en z más allá de la banda.

- [ ] **Paso 1: escribir las pruebas que fallan** (marcadas `#[ignore]`):
  - `radial_back_vanes_have_their_strip_volume`: el volumen se aproxima a
    `count·thickness·Σ(altura·dr)` de la banda, con 2 % de tolerancia.
  - `back_vanes_join_the_impeller`: el volumen del rodete aumenta en el de los álabes,
    con 1e-4 de tolerancia relativa.
  - `curved_back_vanes_build`.
- [ ] **Paso 2:** `cargo test -p tp-cad -- --ignored back_vanes`. Debe fallar.
- [ ] **Paso 3: implementar.**
- [ ] **Paso 4:** `cargo test -p tp-cad -- --ignored back_vanes`. Debe pasar. Si alguna
  prueba nueva tarda demasiado en Windows, se añade a las exclusiones del CI como en
  `90ffadc`.
- [ ] **Paso 5:** commit: `✨ feat(cad): los álabes de rebombeo se unen al rodete`.

### Tarea 13: álabes de rebombeo en la GUI, el meridiano y la CLI

**Archivos:**
- Modificar: `crates/tp-gui/src/steps.rs` (tarjeta), la vista del meridiano (banda
  rayada; el dibujo de los hilos de material está en `crates/tp-gui/src/views/`),
  `crates/tp-gui/src/i18n/es.rs`, `crates/tp-gui/tests/ui.rs` y
  `crates/tp-cli/src/geometry.rs`

**Interfaces:**
- Consume: `BackVaneSettings` y `BackVanes` (tarea 11).
- Produce:
  - tarjeta "Back vanes" / «Álabes de rebombeo», con las filas "Count", "Shape",
    "Outer diameter", "Inner diameter", "Height", "Thickness" y "Tip clearance";
  - al activar los álabes con el sello trasero activo, un aviso en línea con el botón
    "Turn off the rear seal" / «Quitar el sello trasero», que no se aplica solo;
  - claves de la CLI: `back-vanes` = `on|off`, `back-vanes-z`,
    `back-vanes-shape` = `radial|curved:<grados>`, `back-vanes-d-out`,
    `back-vanes-d-in`, `back-vanes-h`, `back-vanes-e` y `back-vanes-gap`.

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - `back_vanes_card_offers_to_turn_off_the_rear_seal`;
  - `the_meridian_draws_the_back_vane_band`;
  - `back_vane_keys_set_the_vanes` (CLI).
- [ ] **Paso 2:** `cargo test -p tp-gui --test ui back_vane` y `cargo test -p tp-cli
  back_vane`. Debe fallar.
- [ ] **Paso 3: implementar.** La banda se dibuja con trazo y relleno rayado, sin cambiar
  de tamaño al pasar el ratón.
- [ ] **Paso 4:** `cargo test -p tp-gui -p tp-cli`, más las capturas `--ignored`
  revisadas y las compilaciones sin características por defecto. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(gui): tarjeta y dibujo de los álabes de rebombeo`.

### Tarea 14: efecto de los álabes de rebombeo en el empuje y la potencia

**Archivos:**
- Modificar: `crates/tp-design/src/axial_thrust.rs` (`AxialThrustInput`, `AxialThrust`)
- Modificar: `crates/tp-design/src/losses.rs` (pérdida de los álabes de rebombeo)
- Modificar: `crates/tp-project/src/design.rs` (`with_contour_seals` pasa los álabes)

**Interfaces:**
- Consume: las ecuaciones verificadas en la tarea 10 y `BackVanes` (tarea 11).
- Produce:
  - `AxialThrustInput::back_vanes: Option<BackVaneLaw>`, con
    `pub struct BackVaneLaw { pub inner_diameter: f64, pub outer_diameter: f64, pub
    height: f64, pub gap: f64 }`;
  - `AxialForces::back_vanes: f64`: la reducción de la fuerza trasera, en N;
  - en `losses`, el término de potencia de los álabes con la ecuación de la tarea 10.

- [ ] **Paso 1: escribir las pruebas que fallan:**
  - `back_vanes_lower_the_rear_force`;
  - `back_vane_force_matches_the_reference_equation`, que reproduce a mano un punto
    de la ecuación de la tarea 10 con su número en el comentario;
  - `back_vanes_cost_power`;
  - `without_back_vanes_the_thrust_is_unchanged`.
- [ ] **Paso 2:** `cargo test -p tp-design axial` y `cargo test -p tp-design losses`.
  Debe fallar.
- [ ] **Paso 3: implementar** según la ecuación anotada, citando al autor original en el
  comentario del módulo, como en la cabecera actual de `axial_thrust.rs`.
- [ ] **Paso 4:** `cargo test -p tp-design -p tp-project`. Debe pasar.
- [ ] **Paso 5:** commit: `✨ feat(design): los álabes de rebombeo entran en el empuje
  axial y las pérdidas`.

---

## Al terminar

- [ ] Actualizar el manual privado `notas-estudio/` con los sellos y los álabes de
  rebombeo. Está excluido de git y no se hace commit de él.
- [ ] Ejecutar `cargo test --workspace` y las tres compilaciones sin características
  por defecto.
