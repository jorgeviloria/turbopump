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
| Número | 4 si d₂ < 200 mm, si no 6 | Gülich tabla 9.3 (ver «Fuentes verificadas») |
| Forma | radiales rectos | Gülich §9.2.7 |
| Diámetro exterior | 0.80·d₂ | Gülich tabla 9.3 |
| Diámetro interior | saliente del eje + t | elección nuestra |
| Altura h | 0.04·r₂ | Gülich tabla 9.3 |
| Espesor | 2·h | Gülich tabla 9.3 |
| Holgura de punta | 0.2·h (= 0.008·r₂) | Gülich tabla 9.3 |

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
  la presión sobre el disco trasero baja: Gülich ecs. T9.3.2, T9.3.3 y T9.2.8 (ver
  «Fuentes verificadas», punto 2).
- **Potencia:** los álabes añaden una pérdida por fricción y bombeo: Gülich ec. T9.3.4,
  en lugar del rozamiento de disco de la cara trasera (punto 3).

## Fuentes verificadas

Verificado el 2026-10-25 sobre los PDF (ecuaciones leídas como imagen). Páginas como
«libro / PDF». Las fórmulas van con nuestros símbolos y en nuestras palabras; no se
copian tablas ni figuras.

Libros: Gülich, *Centrifugal Pumps*, 2.ª ed. (2010), PDF = libro + 35; Stepanoff,
*Centrifugal and Axial Flow Pumps*, 2.ª ed. (1957), escaneo a doble página, PDF ≈ 7 +
libro/2; Lobanoff y Ross, *Centrifugal Pumps: Design and Application*, 2.ª ed., PDF =
libro + 15.

Símbolos: z_v número de álabes, h altura axial del álabe, s_v holgura axial entre la
punta del álabe y la carcasa, d_Rs diámetro exterior de los álabes, r₂ = d₂/2, u₂ = ω·r₂,
Re_u = u₂·r₂/ν, k = c_u/(ω·r) factor de rotación, k_o el de Gülich ec. T9.1.3 (sin fuga).

### 1. Álabes de rebombeo: geometría por defecto (tarea 11)

Gülich resume en la tabla 9.3 los ensayos de la tesis de Möhring (TU Braunschweig,
1976; su referencia [9.4]). La sección 9.2.7 (533 / 568) añade que lo más común es
recortar los álabes (d_Rs < d₂), porque la potencia crece con d_Rs⁵ y la presión solo
con d_Rs²; que la holgura de punta es la mínima que permite la mecánica (tolerancias,
dilatación, deformación); y que los rodetes con álabes radiales suelen ir sin agujeros
de equilibrio.

| Dato | Rango en la fuente | Por defecto (tarea 11) | Fuente |
|---|---|---|---|
| Número z_v | 4 si d₂ < 200 mm; 6 a 8 si d₂ > 200 mm | 4 si d₂ < 200 mm, si no 6 | Gülich tabla 9.3, 534 / 569. Stepanoff da lo mismo: de 4 en bombas pequeñas a 6 en grandes (§11.1 b, 208 / 111) |
| Diámetro exterior d_Rs | d_Rs/d₂ = 0.75 a 0.85 | 0.80·d₂ (sustituye a «borde del disco») | Gülich tabla 9.3 y §9.2.7, 533–534 / 568–569 |
| Altura h | h/r₂ = 0.03 a 0.05 | 0.04·r₂ | Gülich tabla 9.3, 534 / 569 |
| Holgura de punta s_v | s/r₂ = 0.008 a 0.015 y s/h = 0.1 a 0.2 | 0.2·h = 0.008·r₂ (cumple los dos rangos; sustituye a «s_ax de la cámara») | Gülich tabla 9.3, 534 / 569 |
| Espesor (ancho circunferencial) | «ancho del álabe = 2·h» | 2·h (sustituye a «espesor e del álabe principal») | Gülich tabla 9.3, 534 / 569 |
| Forma | radiales rectos, o curvados hacia atrás siguiendo los álabes principales | radiales rectos | Gülich §9.2.7, 533 / 568 |
| Diámetro interior | sin valor (la fig. 9.17 los dibuja hasta el cubo) | saliente del eje + t | **elección nuestra** |

Lobanoff y Ross (§«Closed impeller with front ring and back balancing ribs», 346 / 361)
piden una holgura de punta de 0.015 in (0.38 mm) como máximo en bombas de proceso
pequeñas; con r₂ = 50 mm, 0.008·r₂ = 0.4 mm, así que concuerda. Ellos y Gülich
coinciden en que al agrandar la holgura sube la presión sobre el disco y el empuje.

### 2. Álabes de rebombeo: efecto sobre k y sobre el empuje (tarea 14)

Gülich §9.2.7 y tabla 9.3 (533–534 / 568–569). En la zona con álabes el factor de
rotación casi no depende del radio y el cálculo se hace sin fuga:

- **Ec. T9.3.2:** k_Rs = 1 / [1 + 0.13·(s_v/(s_v + h))·√(r₂/(h·z_v))].
  Crece con z_v y h y baja con s_v. Validez: la geometría de la tabla 9.3, álabes
  radiales. Ejemplo: d₂ = 300 mm, z_v = 6, h = 6 mm, s_v = 1.2 mm → k_Rs ≈ 0.96.
- **Ec. T9.3.3:** factor medio en todo el disco trasero cuando d_Rs < d₂:
  k_av = √{ (d_Rs/d₂)^(2 − 0.9·d_Rs/d₂) · (k_Rs² − k_o²) + k_o² }.
  Con d_Rs = d₂ da k_av = k_Rs; sin álabes, k_o.
- **Presión sobre el disco y en el sello del eje:** Gülich ec. T9.1.7 (524 / 559) con
  k = k_av: p(r) = p₂ − (ρ/2)·u₂²·k_av²·(1 − r²/r₂²).
- **Empuje:** Gülich ec. T9.2.8 (526 / 561), poniendo k̄ = k_av en el lado trasero y
  dejando el delantero como está: F_Hy = π·r₂²·{Δp_La·(x_sp² − x_D²) −
  (ρ/4)·u₂²·[k_av²·(1 − x_D²)² − k̄_FS²·(1 − x_sp²)²]}, con x = d/d₂. Gülich lo indica
  así en la pág. 534 / 569 (ecs. T9.2.3 o T9.2.8 con k_av).

Contraste (no se implementa): Stepanoff, cap. 11 (205–209 / 109–111). Su ec. 11.5
estima la rotación con nervios que no ajustan a la carcasa como ω′ = (ω/2)·(1 + t/s),
con t la altura del nervio y s la distancia total del disco a la carcasa (no la
holgura de punta). Su ec. 11.4 da la reducción del empuje con rotación completa sobre
la corona de los nervios: ΔT = (3/8)·(A_R − A_s)·ρ·(u_R² − u_s²)/2 (u_R en d_Rs, u_s
en el eje). La fig. 16‑7 de Lobanoff y Ross (348 / 363) es esa misma ecuación en
unidades inglesas (0.00253 = (3/8)·γ/(2g) en psi por (ft/s)²). Con los datos del ejemplo,
Stepanoff da k ≈ 0.92 (s = h + s_v = 7.2 mm) frente a 0.96 de Gülich: el orden es el
mismo; usamos Gülich porque separa z_v, h y s_v. Japikse, Marscher y Furst solo lo
tratan de forma cualitativa (§2.3.10, 2‑85 a 2‑86 / 150–151, y §8.10, 8‑20 / 626) y atribuyen a
Stepanoff la preferencia por álabes radiales con poca holgura, que dan rotación casi de
sólido rígido.

Si el sello trasero y los álabes conviven, Gülich no da la fórmula (sus rodetes con
álabes radiales van sin anillo trasero). **Elección nuestra:** aplicar k_av solo por
encima del anillo trasero y avisar de que el cálculo queda fuera del rango ensayado.

### 3. Álabes de rebombeo: potencia (tarea 14)

- **Gülich ec. T9.3.4** (534 / 569), para álabes **radiales**, recortados o no:
  P_Rs = ζ_Rs·ρ·ω³·r₂⁵, con
  ζ_Rs = (0.1/Re_u^0.2)·[ (d_Rs/d₂)⁴·((h + s_v)/r₂ + 0.24) + ¼·((h + s_v)/r₂)^0.1·(1 − (d_Rs/d₂)⁵) ].
  El segundo término cubre la corona lisa entre d_Rs y d₂, así que leemos P_Rs como la
  potencia de **toda** la cara trasera con álabes: en la tarea 14 sustituye al
  rozamiento de disco de la cara trasera (`disk_friction_power` de ese lado), no se suma
  a él. (Esta lectura es **nuestra**; el texto no lo dice con esas palabras. Orden de
  magnitud comprobado: con d_Rs/d₂ = 0.8, (h + s_v)/r₂ = 0.05 y Re_u = 10⁷,
  ζ_Rs ≈ 1.0·10⁻³, frente a ≈ 0.8·10⁻³ de una cara lisa.)
- **Alternativa no usada:** Gülich ec. T3.10.10 (tabla 3.10, 143 / 178) multiplica el
  coeficiente de rozamiento de disco de la ec. T3.10.9 por f_RS = 0.63 + 0.6·d_Rs/d₂
  (f_RS = 1 sin álabes). Es una corrección global, pensada para el escalado por
  viscosidad; por debajo de d_Rs/d₂ ≈ 0.62 da f_RS < 1, sin sentido físico. No se usa a
  la vez que T9.3.4 para no contar dos veces la misma pérdida.
- **Contexto:** Gülich §9.2.7 dice que la pérdida es apreciable por debajo de n_q ≈ 20
  y que con álabes curvados como los principales casi no hay pérdida (T9.3.4 no vale
  para ellos). Stepanoff (cap. 11, 221 / 117) midió unos 2 puntos de rendimiento menos
  en bombas de turbina vertical al añadir nervios que reducían el empuje a la mitad, y
  dice (209 / 111) que la potencia crece con d_Rs² y con h; Gülich, con d_Rs⁵. Nos
  quedamos con Gülich, que es posterior y está basado en ensayos sistemáticos.

### 4. Espesor radial de los anillos postizos e_r, e_c

Buscado sin resultado en Gülich (cap. 3.6 sellos, §14.4.4 materiales de anillos),
Lobanoff y Ross (cap. 12 «Impeller Wear Rings», 213 / 228; cap. 13; cap. 16) y
Stepanoff (cap. 2, cap. 17 dilatación) y Japikse, Marscher y Furst (§2.3.9, fig. 2.82, 2‑81 / 147,
que remite a Karassik para los tipos de anillo; cap. 6). Ninguno da un espesor radial ni una relación con el diámetro. Lo único que aparece es
cualitativo: Stepanoff (cap. 17, ≈ 496 / 255–256) advierte que los anillos montados
apretados en frío pueden aflojarse en caliente por diferencia de dilatación o por el
calor del roce, y Lobanoff y Ross (213 / 228) que a alta velocidad deben fijarse con
mucho cuidado.

**Resultado:** e_r = e_c = máx(0.03·d, 4 mm) sigue siendo **elección nuestra**, sin
fuente. No cambia: `RING_THICKNESS_RATIO` y `MIN_RING_THICKNESS` se quedan como están.

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
