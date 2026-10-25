# Anillos de desgaste y álabes de rebombeo — diseño

Fecha: 2026-10-24 · Estado: aprobado en conversación, pendiente de revisión escrita

## Objetivo

El contorno meridional del rodete cerrado debe traer, por defecto, un **sello delantero**
(en el ojo) y un **sello trasero** (en la cara posterior del disco del cubo), cada uno
formado por un **par de anillos de desgaste postizos**: uno en el rodete y otro en la
carcasa, montados a presión sobre asientos. Las medidas son automáticas y editables, y
cada sello y cada anillo se pueden quitar cuando el diseño no los lleva. Opcionalmente,
la cara trasera lleva **álabes de rebombeo** (pump-out vanes), como en los rodetes de
las bombas ANSI B73.1.

Criterios de éxito:

- Un diseño nuevo muestra los dos sellos con sus cuatro anillos en el meridiano y en la
  CAD, sin tocar nada.
- Cualquier medida se puede fijar a mano (queda marcada «a mano») y volver a automática.
- Desactivar un anillo deja su pieza maciza hasta la holgura; desactivar un sello quita
  asiento, anillos y collar.
- El STEP lleva el rodete y cada anillo como cuerpos separados, con volumen y masa por
  separado.
- El empuje axial usa los diámetros y holguras de los sellos, sin datos duplicados.
- Los proyectos guardados antes siguen abriendo con la misma geometría delantera.

Fuera de alcance: rodetes abiertos y semiabiertos (holgura frontal ajustable), chaflanes
de entrada de los anillos, tolerancias de ajuste en la geometría, sellos de laberinto
escalonados.

## Arquitectura

Dos módulos nuevos en `tp-design/src/impeller/`:

- `seals.rs`: fuente única de los sellos. Lo consumen `material.rs` (asientos del
  rodete), `chambers.rs` (asientos de la carcasa), `tp-cad` (sólidos de los anillos) y
  `axial_thrust.rs` (d_sp, d_s2, s).
- `back_vanes.rs`: los álabes de rebombeo, consumidos por el meridiano, `chambers.rs`,
  `tp-cad` y el empuje axial y las pérdidas.

```rust
pub struct RingSettings {
    pub enabled: bool,
    /// Espesor radial del anillo, m; automático: máx(0.03·d, 4 mm).
    pub thickness: Param<f64>,
}

pub struct SealSettings {
    pub enabled: bool,
    /// Diámetro en la holgura (exterior del anillo del rodete), m.
    pub diameter: Param<f64>,
    /// Largo axial de anillos y asientos, m.
    pub length: Param<f64>,
    /// Holgura radial s, m; automática: Gülich ec. 3.12.
    pub clearance: Param<f64>,
    pub impeller_ring: RingSettings,
    pub casing_ring: RingSettings,
}

pub struct Seals {
    pub front: SealSettings,
    pub rear: SealSettings,
}
```

`Seals::resolve(basis, material)` devuelve, para cada sello activo, los radios usados:

| Superficie | Radio |
|---|---|
| Asiento del rodete | r − e_r (r si el anillo del rodete está desactivado) |
| Anillo del rodete | de r − e_r a r |
| Holgura | de r a r + s |
| Anillo de la carcasa | de r + s a r + s + e_c |
| Asiento de la carcasa | r + s + e_c (r + s si el anillo de la carcasa está desactivado) |

### Compatibilidad

- `MaterialSettings::neck_length` sigue leyéndose: si un proyecto antiguo lo trae, pasa a
  `seals.front.length`. Un `neck_length` = 0 desactiva el sello delantero.
- Los proyectos antiguos sin `seals` toman los valores por defecto, salvo el sello
  trasero y los anillos, que en proyectos antiguos se cargan **desactivados** para no
  cambiar su geometría. Solo los diseños nuevos los traen activos.
- `AxialThrustInput::seal_diameter` y `rear_seal_diameter` pasan a ser lecturas de los
  sellos cuando existen. Si el usuario los había fijado, el valor se migra al sello.

## Geometría

### Sello delantero

- **Rodete:** el cuello del sudario (el de hoy) se tornea a r_sp − e_r a lo largo de L,
  desde la punta. El **tope** es la cara exterior del sudario donde termina el cuello. El
  anillo entra desde la succión.
- **Carcasa:** la pared delantera de la cámara lleva un asiento a r_sp + s + e_c, con el
  tope en el lado de la succión. El anillo entra desde el lado del rodete.

### Sello trasero

- **Rodete:** un **collar** sale de la cara trasera del disco hacia atrás, de largo L₂.
  Su exterior es el asiento a r_s2 − e_r y su pared radial tiene el espesor t del disco.
  El tope del anillo es la cara trasera.
- **Carcasa:** la pared trasera de la cámara lleva el asiento a r_s2 + s + e_c.
- Dentro del collar queda la cámara de equilibrio. Los agujeros de equilibrio (ya en
  `tp-cad`) se sitúan entre el saliente del eje y el collar.

### Valores por defecto

| Dato | Delantero | Trasero | Origen |
|---|---|---|---|
| Activo | sí | sí | requisito |
| Diámetro | d_sp = d₁ + 2t | d_s2 = d_sp | actual; d_s2 = d_sp equilibra (Gülich 9.2.2) |
| Largo L | 0.12·d_sp | 0.12·d_s2 | Gülich 3.6.2 |
| Holgura s | Gülich ec. 3.12 | igual | actual |
| e_r, e_c | máx(0.03·d, 4 mm) | igual | elección nuestra, a contrastar antes de fijar |
| Anillos rodete / carcasa | sí / sí | sí / sí | requisito |

### Avisos (no bloquean el diseño)

- Pared bajo el asiento del rodete (r_sp − e_r − r_ojo) menor que el espesor t.
- Tope con altura menor que e_r (el sudario no llega a ese radio).
- Collar trasero que choca con el saliente del eje.
- Agujeros de equilibrio que no caben entre el saliente y el collar.
- Holgura s por debajo de la mínima de Gülich.

El ajuste a presión (p. ej. H7/s6) no cambia la geometría: los anillos van en diámetros
nominales y el ajuste queda como nota de fabricación.

## Álabes de rebombeo

```rust
pub enum VaneShape { Radial, Curved { angle: f64 } }

pub struct BackVaneSettings {
    pub enabled: bool,
    pub count: Param<u32>,
    pub shape: VaneShape,
    pub outer_diameter: Param<f64>,
    pub inner_diameter: Param<f64>,
    pub height: Param<f64>,
    pub thickness: Param<f64>,
    /// Holgura axial entre la punta del álabe y la carcasa, m.
    pub tip_clearance: Param<f64>,
}
```

| Dato | Por defecto | Origen |
|---|---|---|
| Activo | no | opcional |
| Número | 6 | a contrastar (Gülich cap. 9.2, Stepanoff, Lobanoff) |
| Forma | radiales rectos | — |
| Diámetro exterior | borde del disco del cubo (d₂ o d₂i) | — |
| Diámetro interior | saliente del eje + t | elección nuestra |
| Altura h | a fijar tras verificar en la literatura | — |
| Espesor | el espesor e del álabe principal | elección nuestra |
| Holgura de punta | s_ax de la cámara trasera | — |

Ningún valor sin fuente se fija hasta verificarlo en los PDF.

- **Convivencia con el sello trasero:** al activar los álabes con el sello trasero
  activo, la GUI ofrece desactivarlo (configuración ANSI) sin obligar. Si se dejan los
  dos, los álabes se recortan por fuera del anillo de la carcasa trasero, con un aviso.
- **Meridiano:** se dibujan como una banda rayada de altura h detrás de la cara trasera,
  entre d_int y d_ext.
- **CAD:** se revoluciona la banda (cara trasera → cara trasera desplazada h) y se
  intersecta con z_v prismas en planta (rectos o en espiral logarítmica de ángulo
  β_v). El resultado se une al rodete. Funciona con la cara trasera curva.
- **Cámara trasera:** en la zona de los álabes, su pared queda a h + holgura de punta
  de la cara del disco.
- **Empuje axial:** en la zona de los álabes, el factor de rotación k se acerca a 1 y
  la presión sobre el disco trasero baja. La fórmula se toma de Gülich cap. 9.2 o de
  Stepanoff, tras verificarla en el PDF.
- **Potencia:** los álabes añaden una pérdida por fricción y bombeo. Entra en la
  predicción de rendimiento con una fuente verificada.

## CAD y exportación

- Los anillos se revolucionan desde sus rectángulos meridionales: hasta cuatro sólidos.
- El STEP exporta el rodete y cada anillo como cuerpos con nombre: «anillo rodete
  delantero», «anillo carcasa delantero», etc.
- El informe da volumen y masa del rodete y de cada anillo por separado.
- Los anillos de la carcasa se exportan con las cámaras laterales cuando estas están
  activas, y como cuerpos sueltos cuando no lo están.

## GUI y CLI

- En el paso de material del Meridiano hay tres tarjetas: **Sello delantero**, **Sello
  trasero** (incluye los agujeros de equilibrio) y **Álabes de rebombeo**. Cada una tiene
  su casilla, y cada anillo la suya.
- Unidades fuera de los campos, nada se mueve al pasar el ratón o al seleccionar, y el
  contorno se actualiza en vivo.
- La casilla de agujeros de equilibrio de Rendimiento pasa a leer el sello trasero.
- Textos en inglés con traducción en `i18n/es.rs`.
- CLI: claves `front_seal.*`, `rear_seal.*` y `back_vanes.*` en el comando de
  geometría.

## Pruebas

- `seals.rs`: radios resueltos con anillos activados y desactivados; migración de
  `neck_length`; avisos de pared, tope y holgura.
- `material.rs` y `chambers.rs`: los hilos automáticos cierran y no se cruzan, con y sin
  cada anillo; la holgura entre los hilos del rodete y de la carcasa vale s.
- `tp-cad`: el volumen del rodete más el de sus anillos coincide con el del rodete
  macizo; los anillos no se intersectan con el rodete ni con la carcasa; la exportación
  incluye los cuerpos con sus nombres.
- `back_vanes.rs`: el volumen de los álabes es z_v·espesor·altura·largo en planta; el
  recorte contra el sello trasero.
- `axial_thrust.rs`: los resultados de hoy se mantienen cuando los sellos repiten los
  valores automáticos actuales.
- GUI: capturas con kittest y pruebas de estabilidad (nada se mueve al pasar el ratón).
- Antes de hacer commit, compilar sin las características por defecto (`tp-cli`,
  `tp-gui`, `tp-fluids`).

## Orden de implementación

1. `seals.rs`: modelo, resolución de radios, migración y pruebas.
2. Asientos en `material.rs` y `chambers.rs`, con sus avisos.
3. Anillos como sólidos en `tp-cad` y en la exportación STEP.
4. GUI y CLI de los sellos.
5. Empuje axial leyendo de los sellos.
6. `back_vanes.rs`: meridiano, cámara, CAD, GUI y CLI.
7. Efecto de los álabes de rebombeo en el empuje y la potencia, tras verificar las
   fórmulas.
